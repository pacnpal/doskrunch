use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::archive::{read_trailer, Algorithm, Archive, FileEntry, TRAILER_SIZE};

pub struct UnpackOptions {
    pub input: PathBuf,
    pub dest: PathBuf,
}

pub fn unpack(opts: UnpackOptions) -> Result<()> {
    let archive = load_archive(&opts.input)?;
    fs::create_dir_all(&opts.dest)
        .with_context(|| format!("create {}", opts.dest.display()))?;

    for entry in &archive.files {
        let stored = entry.display_name();
        let safe = safe_basename(&stored)?;
        let out = opts.dest.join(safe);
        write_entry(&out, entry, archive.algorithm)?;
    }
    Ok(())
}

/// Reject stored names containing path separators, `..`, NULs, leading
/// dots, empty strings, or Windows reserved device names. The host
/// always writes 8.3 ASCII names, but `unpack` may be fed a hostile
/// archive — keep it strictly basename-only and cross-platform safe.
fn safe_basename(name: &str) -> Result<&str> {
    if name.is_empty() {
        bail!("archive entry has empty name");
    }
    if name == "." || name == ".." {
        bail!("archive entry name '{}' is reserved", name);
    }
    for b in name.bytes() {
        match b {
            b'/' | b'\\' | 0 => bail!("archive entry name '{}' contains path separator or NUL", name),
            b if b < 0x20 => bail!("archive entry name '{}' contains control character", name),
            _ => {}
        }
    }
    if name.starts_with('.') {
        bail!("archive entry name '{}' has a leading dot", name);
    }
    // Windows trims trailing dots and spaces, so "CON.", "CON ", "NUL  "
    // all resolve to the same reserved device. Reject either as input.
    if name.ends_with('.') || name.ends_with(' ') {
        bail!(
            "archive entry name '{}' has trailing dot/space (would resolve to a device on Windows)",
            name
        );
    }
    // Windows reserved device names — opening these can hang or write
    // to a device instead of a file. Compare on the stem before the
    // first dot, case-insensitively, after trimming trailing dots/spaces.
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let trimmed = name.trim_end_matches(['.', ' ']);
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    for r in RESERVED {
        if stem.eq_ignore_ascii_case(r) {
            bail!("archive entry name '{}' is a reserved device name", name);
        }
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::safe_basename;

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_basename("../etc/passwd").is_err());
        assert!(safe_basename("a/b").is_err());
        assert!(safe_basename("a\\b").is_err());
    }

    #[test]
    fn rejects_reserved_device_names() {
        for bad in &["CON", "con", "Nul", "COM1.TXT", "lpt9.dat", "AUX"] {
            assert!(safe_basename(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn rejects_trailing_dot_or_space() {
        for bad in &["CON.", "CON ", "NUL  ", "LPT1.", "FILE."] {
            assert!(safe_basename(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn accepts_plain_8_3() {
        assert!(safe_basename("HELLO.TXT").is_ok());
        assert!(safe_basename("README").is_ok());
    }
}

pub fn load_archive(path: &Path) -> Result<Archive> {
    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let total = f.metadata()?.len();
    if total < TRAILER_SIZE as u64 {
        bail!("{}: too small to contain a doskrunch trailer", path.display());
    }
    f.seek(SeekFrom::End(-(TRAILER_SIZE as i64)))?;
    let mut tail = [0u8; TRAILER_SIZE];
    f.read_exact(&mut tail)?;
    let archive_offset = read_trailer(&tail)
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;
    f.seek(SeekFrom::Start(archive_offset as u64))?;
    let archive =
        Archive::read(&mut f).map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;
    Ok(archive)
}

fn write_entry(out: &Path, entry: &FileEntry, algo: Algorithm) -> Result<()> {
    if !matches!(algo, Algorithm::Stored) {
        bail!(
            "algorithm '{}' decode not yet supported on host (phase {} territory)",
            algo.name(),
            match algo {
                Algorithm::Aplib => "2",
                Algorithm::Lzsa2 => "6",
                Algorithm::Lzma => "5",
                Algorithm::Stored => "?",
            }
        );
    }
    // Phase 1 hard cap on per-file uncompressed size. The current unpack
    // path builds the whole file in memory; streaming lands in Phase 4
    // along with chunked extraction (PLAN.md §10). Until then, refuse
    // to materialise more than 256 MiB per entry — well above any
    // realistic Phase 1 fixture, far below OOM territory.
    const MAX_UNCOMPRESSED: u32 = 256 * 1024 * 1024;
    let total = entry.uncompressed_size();
    if total > MAX_UNCOMPRESSED {
        bail!(
            "{}: declared uncompressed size {} exceeds the phase-1 unpack cap ({} bytes); \
             this lands in phase 4 with streaming extraction.",
            entry.display_name(),
            total,
            MAX_UNCOMPRESSED
        );
    }
    let mut data = Vec::with_capacity(total as usize);
    for c in &entry.chunks {
        if c.data.len() != c.uncompressed_size as usize {
            bail!(
                "{}: stored chunk size mismatch ({} vs declared {})",
                entry.display_name(),
                c.data.len(),
                c.uncompressed_size
            );
        }
        data.extend_from_slice(&c.data);
    }
    let crc = crc32fast::hash(&data);
    if crc != entry.crc32 {
        bail!(
            "{}: CRC mismatch (stored {:#010x}, actual {:#010x})",
            entry.display_name(),
            entry.crc32,
            crc
        );
    }
    // No parent-dir creation: `safe_basename` rejects path separators,
    // so `out` is always `<dest>/<basename>`. `dest` was created in unpack().
    fs::write(out, &data).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}
