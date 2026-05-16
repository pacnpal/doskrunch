use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::archive::{build_stored_entry, flags, Algorithm, Archive, TargetTier};
use crate::fat_time::unix_to_fat;
use crate::name83::{dedupe, mangle};
use crate::stubs::stub_for;

pub struct PackOptions {
    pub output: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub algorithm: Algorithm,
    pub target: TargetTier,
    pub preserve_timestamps: bool,
}

pub fn pack(opts: PackOptions) -> Result<()> {
    if matches!(opts.algorithm, Algorithm::Lzma)
        && matches!(opts.target, TargetTier::I8086 | TargetTier::I286)
    {
        bail!("LZMA requires 386 or later; pick --target 386 or higher.");
    }
    if !matches!(opts.algorithm, Algorithm::Stored) {
        bail!(
            "algorithm '{}' not yet supported in this phase; only 'stored' is available",
            opts.algorithm.name()
        );
    }

    let stub = stub_for(opts.algorithm, opts.target)
        .map_err(|e| anyhow::anyhow!(e))?;
    if stub.len() < 2 || &stub[..2] != b"MZ" {
        bail!(
            "embedded stub blob for ({}, {}) is not a DOS .EXE (missing MZ magic) — \
             this build was made before the Watcom stub was produced; \
             build it via `docker run --rm -v \"$PWD/stubs:/work\" -w /work doskrunch-watcom make all` \
             and commit the result to `stubs/blobs/`.",
            opts.algorithm.name(),
            opts.target.name()
        );
    }

    let mut archive = Archive::new(opts.algorithm, opts.target);
    if !opts.preserve_timestamps {
        archive.flags |= flags::REPRODUCIBLE;
    }

    if opts.inputs.len() > u16::MAX as usize {
        bail!(
            "too many input files ({}); archive header file_count is u16",
            opts.inputs.len()
        );
    }

    let mut used: HashSet<String> = HashSet::new();
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for src in &opts.inputs {
        let meta = fs::metadata(src)
            .with_context(|| format!("stat {}", src.display()))?;
        if !meta.is_file() {
            bail!(
                "{}: not a regular file (directory walking lands in phase 4)",
                src.display()
            );
        }
        let src_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("non-utf8 filename: {}", src.display()))?;
        let (mangled, was_mangled) = mangle(src_name);
        let final_name = dedupe(&mangled, &used).with_context(|| {
            format!(
                "{}: exhausted the ~1..~9999 suffix space for stem '{}'",
                src.display(),
                mangled
            )
        })?;
        if was_mangled || final_name != src_name.to_ascii_uppercase() {
            eprintln!(
                "warning: '{}' stored as '{}' (8.3 mangling)",
                src_name, final_name
            );
        }
        used.insert(final_name.clone());
        entries.push((final_name, src.clone()));
    }

    // Reproducible default: sort lexicographically by stored name.
    if !opts.preserve_timestamps {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
    }

    for (stored_name, src) in entries {
        let data = fs::read(&src).with_context(|| format!("read {}", src.display()))?;
        let timestamp = if opts.preserve_timestamps {
            let meta = fs::metadata(&src)?;
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            unix_to_fat(mtime_secs)
        } else {
            0
        };
        if data.len() > u32::MAX as usize {
            bail!("{}: file exceeds 4 GiB", src.display());
        }
        let entry = build_stored_entry(&stored_name, 0x20, timestamp, &data)
            .map_err(|e| anyhow::anyhow!("{}: {}", src.display(), e))?;
        archive.files.push(entry);
    }

    // Cumulative size must fit in the u32 header fields. DOS lseek is
    // signed 32-bit, so the whole .EXE must also fit in 2 GiB (i32::MAX).
    let mut total_u: u64 = 0;
    let mut total_c: u64 = 0;
    for f in &archive.files {
        total_u += f.uncompressed_size() as u64;
        total_c += f.compressed_size() as u64;
    }
    if total_u > u32::MAX as u64 || total_c > u32::MAX as u64 {
        bail!(
            "archive payload exceeds 4 GiB (uncompressed {} / compressed {})",
            total_u,
            total_c
        );
    }
    // i32::MAX accounts for the stub + archive + trailer; checked again
    // in write_sfx once we know the exact archive byte size.
    write_sfx(&opts.output, stub, &archive)?;
    Ok(())
}

fn write_sfx(out: &Path, stub: &[u8], archive: &Archive) -> Result<()> {
    let mut f = fs::File::create(out).with_context(|| format!("create {}", out.display()))?;
    f.write_all(stub)?;
    let archive_offset: u32 = stub.len().try_into().context("stub larger than 4 GiB")?;
    archive.write(&mut f)?;
    crate::archive::write_trailer(&mut f, archive_offset)?;
    let total = f.metadata()?.len();
    // DOS lseek is signed 32-bit, so the stub can't reach past 2 GiB
    // even though the archive header's u32 fields could express more.
    if total > i32::MAX as u64 {
        bail!(
            "output .EXE is {} bytes; DOS lseek caps at 2 GiB",
            total
        );
    }
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_input(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body).unwrap();
        p
    }

    #[test]
    fn rejects_lzma_on_8086() {
        let td = tempfile::tempdir().unwrap();
        let input = make_input(td.path(), "a.txt", b"x");
        let opts = PackOptions {
            output: td.path().join("o.exe"),
            inputs: vec![input],
            algorithm: Algorithm::Lzma,
            target: TargetTier::I8086,
            preserve_timestamps: false,
        };
        let err = pack(opts).unwrap_err();
        assert!(err.to_string().contains("LZMA requires 386"));
    }

    #[test]
    fn rejects_directory_input() {
        let td = tempfile::tempdir().unwrap();
        let subdir = td.path().join("d");
        fs::create_dir(&subdir).unwrap();
        let opts = PackOptions {
            output: td.path().join("o.exe"),
            inputs: vec![subdir],
            algorithm: Algorithm::Stored,
            target: TargetTier::I8086,
            preserve_timestamps: false,
        };
        let err = pack(opts).unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }
}
