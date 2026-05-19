use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use doskrunch::archive::{APLIB_CHUNK_INPUT, LZMA_CHUNK_INPUT};
use doskrunch::{archive, inspect, pack, unpack};

#[derive(Parser)]
#[command(name = "doskrunch", about = "Build self-extracting DOS .EXE archives.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a self-extracting DOS .EXE from the given input files.
    ///
    /// Directory inputs are walked recursively; only regular files are
    /// included. Symlinks are skipped (whether named as a top-level
    /// input or found during the walk). Files extract into the current
    /// directory at runtime regardless of their position in the source
    /// tree (flat extraction); two files with the same basename across
    /// different subdirectories get FAT-style `~N` dedupe suffixes.
    /// The output path itself is excluded from the walk so re-running
    /// `pack dir/OUT.EXE dir/` doesn't pack a previous SFX into the
    /// new one.
    Pack {
        /// Path of the .EXE to write.
        output: PathBuf,
        /// Input files or directories. Directories are walked
        /// recursively for regular files.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Compression algorithm. Defaults to `aplib`; `stored`
        /// remains available as a fallback. LZSA2 and LZMA land in
        /// later phases.
        #[arg(long, value_enum, default_value_t = AlgoArg::Aplib)]
        algo: AlgoArg,
        /// CPU target tier for the embedded stub.
        #[arg(long, value_enum, default_value_t = TargetArg::I8086)]
        target: TargetArg,
        /// Preserve source mtimes instead of zeroing them. Opt-out of the
        /// default reproducible-build behaviour.
        #[arg(long)]
        preserve_timestamps: bool,
        /// Max uncompressed bytes per chunk. Algorithm-dependent
        /// ceiling: aplib ≤ 16384 (the 8086 stub's BSS scratch);
        /// stored ≤ 65535 (the per-chunk u16 size field). Default
        /// 16384. Today pack reads each input fully into memory before
        /// encoding, so this knob controls archive layout (chunk count,
        /// per-chunk framing overhead) and the transient encode buffer,
        /// not peak host RAM; the stub's RAM is bounded by its BSS
        /// regardless of the value here.
        #[arg(long, default_value_t = APLIB_CHUNK_INPUT)]
        chunk_size: usize,
    },
    /// Extract a doskrunch SFX on the host (no DOS required).
    Unpack {
        input: PathBuf,
        #[arg(short = 'd', long = "dest", default_value = ".")]
        dest: PathBuf,
    },
    /// Print archive metadata (header, file list, sizes).
    Inspect { input: PathBuf },
    /// List the CPU target tiers shipped in this build.
    ListTargets,
    /// List the compression algorithms shipped in this build.
    ListAlgos,
}

#[derive(Copy, Clone, ValueEnum)]
enum AlgoArg {
    Stored,
    Aplib,
    Lzsa2,
    Lzma,
}
impl AlgoArg {
    fn to_archive(self) -> archive::Algorithm {
        match self {
            Self::Stored => archive::Algorithm::Stored,
            Self::Aplib => archive::Algorithm::Aplib,
            Self::Lzsa2 => archive::Algorithm::Lzsa2,
            Self::Lzma => archive::Algorithm::Lzma,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
#[clap(rename_all = "lowercase")]
enum TargetArg {
    #[clap(name = "8086")]
    I8086,
    #[clap(name = "286")]
    I286,
    #[clap(name = "386")]
    I386,
    #[clap(name = "486")]
    I486,
    Pentium,
    #[clap(name = "pentium-mmx")]
    PentiumMmx,
    P2,
    P3,
}
impl TargetArg {
    fn to_archive(self) -> archive::TargetTier {
        match self {
            Self::I8086 => archive::TargetTier::I8086,
            Self::I286 => archive::TargetTier::I286,
            Self::I386 => archive::TargetTier::I386,
            Self::I486 => archive::TargetTier::I486,
            Self::Pentium => archive::TargetTier::Pentium,
            Self::PentiumMmx => archive::TargetTier::PentiumMmx,
            Self::P2 => archive::TargetTier::P2,
            Self::P3 => archive::TargetTier::P3,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Pack {
            output,
            inputs,
            algo,
            target,
            preserve_timestamps,
            chunk_size,
        } => {
            // Validate `--chunk-size` only for shipped algorithms.
            // `aplib`, `stored`, and `lzma` are all shipped now
            // (Phase 5 wired LZMA), so each has its own ceiling. The
            // remaining deferred algorithm is `lzsa2`; for that we
            // return None so the chunk-size check is skipped and the
            // deferred-algorithm bail inside pack() runs first,
            // producing a more useful error than a chunk-size
            // complaint against a placeholder ceiling would.
            let max_chunk: Option<usize> = match algo {
                AlgoArg::Aplib => Some(APLIB_CHUNK_INPUT),
                AlgoArg::Stored => Some(u16::MAX as usize),
                AlgoArg::Lzma => Some(LZMA_CHUNK_INPUT),
                AlgoArg::Lzsa2 => None,
            };
            if let Some(max) = max_chunk {
                if !(1..=max).contains(&chunk_size) {
                    bail!(
                        "--chunk-size must be in 1..={} for --algo {} (got {})",
                        max,
                        algo.to_archive().name(),
                        chunk_size
                    );
                }
            }
            pack::pack(pack::PackOptions {
                output,
                inputs,
                algorithm: algo.to_archive(),
                target: target.to_archive(),
                preserve_timestamps,
                chunk_size,
            })
        }
        Cmd::Unpack { input, dest } => unpack::unpack(unpack::UnpackOptions { input, dest }),
        Cmd::Inspect { input } => inspect::inspect(inspect::InspectOptions { input }),
        Cmd::ListTargets => {
            println!("8086         shipped (default)");
            println!("286          shipped");
            println!("386          shipped (perf gate pending; see tests/benchmarks/results.md)");
            println!("486          shipped");
            println!("pentium      shipped (perf gate pending; see tests/benchmarks/results.md)");
            println!("pentium-mmx  shipped (MMX 8-byte block copy in depacker)");
            println!("p2           shipped (P6 codegen + MMX depacker)");
            println!("p3           shipped (P6 codegen + MMX depacker; SSE depacker variant deferred)");
            Ok(())
        }
        Cmd::ListAlgos => {
            println!("aplib        shipped (default; via vendored apultra)");
            println!("stored       shipped (fallback / no-op baseline)");
            println!("lzma         shipped (best ratio; --target 386+ only)");
            println!("lzsa2        planned (phase 6; fast decompression)");
            Ok(())
        }
    }
}
