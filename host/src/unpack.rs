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
/// dots, or empty strings. The host always writes 8.3 ASCII names, but
/// `unpack` may be fed a hostile archive — keep it strictly basename-only.
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
    Ok(name)
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
    // Cap the prealloc so a hostile archive can't trigger huge allocs.
    // 16 MiB matches what a single u16-bounded chunk can produce in
    // realistic Phase 1 packs.
    let total = entry.uncompressed_size();
    let prealloc = std::cmp::min(total as usize, 16 * 1024 * 1024);
    let mut data = Vec::with_capacity(prealloc);
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
