use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use doskrunch::archive::{APLIB_CHUNK_INPUT, LZMA_CHUNK_INPUT, LZSA2_CHUNK_INPUT};
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
        /// Compression algorithm. Defaults to `aplib`. `stored` is the
        /// no-op fallback. `lzma` is shipped on `--target 386+` for
        /// the best ratio. `lzsa2` is shipped on every tier for the
        /// fastest decompression on retro hardware.
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
        /// Optional command line invoked via INT 21h/4Bh after the
        /// SFX finishes extracting. Plain DOS argv: 8.3 program name
        /// optionally followed by a space and args (e.g.
        /// `"SETUP.EXE /Q"`). Typically the program is one of the
        /// extracted files; pack doesn't enforce that, since DOS
        /// resolves the name at extract time against the current
        /// directory and PATH. Capped at 127 printable-ASCII bytes
        /// (the stub's RUN_AFTER_BUF cap). Pack also fails if the
        /// cumulative archive prefix (25-byte header + all per-file
        /// records, INCLUDING the per-chunk compressed data) exceeds
        /// 65535 bytes — the on-disk `run_after_offset` is a u16,
        /// and chunk data is stored inline with each file's record.
        /// In practice this caps the compressed archive at roughly
        /// 64 KiB when `--run-after` is set.
        #[arg(long)]
        run_after: Option<String>,
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
            run_after,
        } => {
            // All four algorithms ship as of Phase 6, so every arm
            // returns a Some(<ceiling>) and the chunk-size check
            // always runs at the CLI layer. If a future algorithm
            // lands as deferred, that arm should return None so the
            // pack()-level deferred-algorithm bail surfaces first.
            let max_chunk: Option<usize> = match algo {
                AlgoArg::Aplib => Some(APLIB_CHUNK_INPUT),
                AlgoArg::Stored => Some(u16::MAX as usize),
                AlgoArg::Lzma => Some(LZMA_CHUNK_INPUT),
                AlgoArg::Lzsa2 => Some(LZSA2_CHUNK_INPUT),
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
                run_after,
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
            println!("lzsa2        shipped (fast decompression; via vendored lzsa)");
            Ok(())
        }
    }
}
