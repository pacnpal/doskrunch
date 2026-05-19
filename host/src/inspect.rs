use std::path::PathBuf;

use anyhow::Result;

use crate::archive::flags;
use crate::unpack::load_archive;

pub struct InspectOptions {
    pub input: PathBuf,
}

pub fn inspect(opts: InspectOptions) -> Result<()> {
    let archive = load_archive(&opts.input)?;
    let (tu, tc) = archive.totals();
    println!("file         : {}", opts.input.display());
    println!("version      : {}", archive.version);
    println!("algorithm    : {}", archive.algorithm.name());
    println!("target tier  : {}", archive.target.name());
    println!(
        "flags        : 0x{:04x}{}{}",
        archive.flags,
        if archive.flags & flags::REPRODUCIBLE != 0 {
            " reproducible"
        } else {
            ""
        },
        if archive.flags & flags::RUN_AFTER != 0 {
            " run-after"
        } else {
            ""
        },
    );
    println!("file count   : {}", archive.files.len());
    println!("uncompressed : {} bytes", tu);
    println!("compressed   : {} bytes", tc);
    let ratio = if tu == 0 {
        0.0
    } else {
        (tc as f64) / (tu as f64) * 100.0
    };
    println!("ratio        : {:.2}% of original", ratio);
    println!();
    // Per-file table. Chunk count is useful for diagnosing per-chunk
    // bugs (e.g. multi-chunk decode regressions); it's bounded by u16
    // so the column never widens past 5 digits.
    println!(
        "{:<14}  {:>5}  {:>10}  {:>10}  {:>10}",
        "name", "chunk", "usize", "csize", "crc32"
    );
    for f in &archive.files {
        println!(
            "{:<14}  {:>5}  {:>10}  {:>10}  {:08x}",
            f.display_name(),
            f.chunks.len(),
            f.uncompressed_size(),
            f.compressed_size(),
            f.crc32
        );
    }
    Ok(())
}
