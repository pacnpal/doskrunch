//! 8.3 filename mangling for FAT.
//!
//! Phase 1 mangles aggressively: uppercase, strip illegal chars, truncate
//! to 8.3, collision-rename with `~N` suffix. `pack` emits a single-line
//! stderr warning whenever the stored name differs from the uppercased
//! source name.

use std::collections::HashSet;
use std::path::Path;

/// Byte values forbidden in a FAT 8.3 basename (also covers the
/// Windows-illegal set `* ? " < > |`, so unpack and parse-time
/// validators can reuse this and stay aligned with what the mangler
/// emits.
pub const ILLEGAL: &[u8] = b" \"*+,/:;<=>?[\\]|";

fn sanitize_part(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max);
    for ch in s.chars() {
        if out.len() >= max {
            break;
        }
        let c = ch.to_ascii_uppercase();
        let b = c as u32;
        if b > 0x7e || (b as u8) < 0x20 {
            // Reject non-ASCII, control bytes < 0x20, and DEL (0x7F).
            out.push('_');
            continue;
        }
        let bb = b as u8;
        if ILLEGAL.contains(&bb) || bb == b'.' {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    out
}

/// Mangle a path-derived filename to FAT 8.3. Returns the mangled name and
/// `true` if it differs from a clean uppercase of the source.
pub fn mangle(src: &str) -> (String, bool) {
    let p = Path::new(src);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("UNNAMED");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    let stem83 = sanitize_part(stem, 8);
    let ext83 = sanitize_part(ext, 3);
    let stem83 = if stem83.is_empty() {
        "UNNAMED".to_string()
    } else {
        stem83
    };
    let name = if ext83.is_empty() {
        stem83.clone()
    } else {
        format!("{stem83}.{ext83}")
    };
    let was_mangled = name != src.to_ascii_uppercase();
    (name, was_mangled)
}

/// Resolve a collision by appending `~N` (DOS-style) to the stem. Returns
/// `None` when every `~1`..`~9999` suffix on this stem is already taken;
/// callers should fail the pack rather than silently overwriting.
pub fn dedupe(name: &str, used: &HashSet<String>) -> Option<String> {
    if !used.contains(name) {
        return Some(name.to_string());
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    for n in 1u32..=9999 {
        let suffix = format!("~{n}");
        let keep = 8usize.saturating_sub(suffix.len());
        let trimmed: String = stem.chars().take(keep).collect();
        let candidate = format!("{trimmed}{suffix}{ext}");
        if !used.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn clean_name_passes() {
        let (n, m) = mangle("READ.ME");
        assert_eq!(n, "READ.ME");
        assert!(!m);
    }

    #[test]
    fn lowercase_uppercased() {
        let (n, m) = mangle("readme.txt");
        assert_eq!(n, "README.TXT");
        assert!(!m, "case change alone is not mangling");
    }

    #[test]
    fn long_truncated() {
        let (n, m) = mangle("verylongfilename.text");
        assert_eq!(n, "VERYLONG.TEX");
        assert!(m);
    }

    #[test]
    fn spaces_become_underscore() {
        let (n, _) = mangle("a b.c");
        assert_eq!(n, "A_B.C");
    }

    #[test]
    fn dedupe_inserts_tilde() {
        let mut used = HashSet::new();
        used.insert("README.TXT".to_string());
        let d = dedupe("README.TXT", &used).unwrap();
        assert_eq!(d, "README~1.TXT");
    }

    #[test]
    fn dedupe_walks() {
        let mut used = HashSet::new();
        used.insert("README.TXT".to_string());
        used.insert("README~1.TXT".to_string());
        let d = dedupe("README.TXT", &used).unwrap();
        assert_eq!(d, "README~2.TXT");
    }

    #[test]
    fn dedupe_truncates_long_stem() {
        let mut used = HashSet::new();
        used.insert("ABCDEFGH.TXT".to_string());
        let d = dedupe("ABCDEFGH.TXT", &used).unwrap();
        assert_eq!(d, "ABCDEF~1.TXT");
    }

    #[test]
    fn dedupe_returns_none_when_exhausted() {
        let mut used = HashSet::new();
        used.insert("AB.TXT".to_string());
        for n in 1..=9999 {
            let suffix = format!("~{n}");
            let keep = 8 - suffix.len();
            let trimmed: String = "AB".chars().take(keep).collect();
            used.insert(format!("{trimmed}{suffix}.TXT"));
        }
        assert!(dedupe("AB.TXT", &used).is_none());
    }
}
