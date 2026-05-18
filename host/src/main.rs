use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use doskrunch::{archive, inspect, pack, unpack};
use doskrunch::archive::APLIB_CHUNK_INPUT;

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
    /// included. Symlinks are skipped. Files extract into the current
    /// directory at runtime regardless of their position in the source
    /// tree (flat extraction); two files with the same basename across
    /// different subdirectories get FAT-style `~N` dedupe suffixes.
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
        /// Max uncompressed bytes per chunk. Default 16384 (matches the
        /// stub's BSS scratch). Smaller values give finer-grained
        /// progress and slightly lower host RAM during pack at a small
        /// compression-ratio cost; larger values are rejected because
        /// the 16-bit stub's small-model data segment can't hold them.
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
            // Validate at the CLI boundary so the user gets a clean
            // error message rather than the library-layer assert.
            let max_chunk = match algo {
                AlgoArg::Aplib => APLIB_CHUNK_INPUT,
                AlgoArg::Stored => u16::MAX as usize,
                AlgoArg::Lzsa2 | AlgoArg::Lzma => APLIB_CHUNK_INPUT,
            };
            if !(1..=max_chunk).contains(&chunk_size) {
                bail!(
                    "--chunk-size must be in 1..={} for --algo {} (got {})",
                    max_chunk,
                    algo.to_archive().name(),
                    chunk_size
                );
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
            println!("286          planned (phase 5)");
            println!("386          shipped (perf gate pending; see tests/benchmarks/results.md)");
            println!("486          planned (phase 5)");
            println!("pentium      shipped (perf gate pending; see tests/benchmarks/results.md)");
            println!("pentium-mmx  planned (phase 5)");
            println!("p2           planned (phase 5)");
            println!("p3           planned (phase 5)");
            Ok(())
        }
        Cmd::ListAlgos => {
            println!("aplib        shipped (default; via vendored apultra)");
            println!("stored       shipped (fallback / no-op baseline)");
            println!("lzsa2        planned (phase 6; fast decompression)");
            println!("lzma         planned (phase 5; best ratio, 386+ only)");
            Ok(())
        }
    }
}
