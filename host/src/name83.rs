//! 8.3 filename mangling for FAT.
//!
//! Phase 1 mangles aggressively: uppercase, strip illegal chars, truncate
//! to 8.3, collision-rename with `~N` suffix. Phase 4 will warn on stderr
//! when mangling happens; we keep the API ready for that.

use std::collections::HashSet;
use std::path::Path;

const ILLEGAL: &[u8] = b" \"*+,/:;<=>?[\\]|";

fn sanitize_part(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max);
    for ch in s.chars() {
        if out.len() >= max {
            break;
        }
        let c = ch.to_ascii_uppercase();
        let b = c as u32;
        if b > 0x7f || (b as u8) < 0x20 {
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

/// Resolve a collision by appending `~N` (DOS-style) to the stem.
pub fn dedupe(name: &str, used: &HashSet<String>) -> String {
    if !used.contains(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    for n in 1u32..=9999 {
        let suffix = format!("~{n}");
        // DOS-style: stem truncated so stem+suffix fits in 8 chars.
        let keep = 8usize.saturating_sub(suffix.len());
        let trimmed: String = stem.chars().take(keep).collect();
        let candidate = format!("{trimmed}{suffix}{ext}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    // Unreachable in practice.
    name.to_string()
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
        // README is 6 chars, ~1 is 2 — fits in 8, so no truncation.
        let d = dedupe("README.TXT", &used);
        assert_eq!(d, "README~1.TXT");
    }

    #[test]
    fn dedupe_walks() {
        let mut used = HashSet::new();
        used.insert("README.TXT".to_string());
        used.insert("README~1.TXT".to_string());
        let d = dedupe("README.TXT", &used);
        assert_eq!(d, "README~2.TXT");
    }

    #[test]
    fn dedupe_truncates_long_stem() {
        let mut used = HashSet::new();
        used.insert("ABCDEFGH.TXT".to_string());
        let d = dedupe("ABCDEFGH.TXT", &used);
        // Keep 6 chars of stem to fit "~1" inside 8.
        assert_eq!(d, "ABCDEF~1.TXT");
    }
}
