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
        let out = opts.dest.join(&stored);
        write_entry(&out, entry, archive.algorithm)?;
    }
    Ok(())
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
    let mut data = Vec::with_capacity(entry.uncompressed_size() as usize);
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
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(out, &data).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}
