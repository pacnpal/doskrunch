use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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
    Pack {
        /// Path of the .EXE to write.
        output: PathBuf,
        /// Input files (directories not yet supported; lands in phase 4).
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Compression algorithm. Phase 1 only ships `stored`; the
        /// default flips to `aplib` once Phase 2 lands.
        #[arg(long, value_enum, default_value_t = AlgoArg::Stored)]
        algo: AlgoArg,
        /// CPU target tier for the embedded stub.
        #[arg(long, value_enum, default_value_t = TargetArg::I8086)]
        target: TargetArg,
        /// Preserve source mtimes instead of zeroing them. Opt-out of the
        /// default reproducible-build behaviour.
        #[arg(long)]
        preserve_timestamps: bool,
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
        } => pack::pack(pack::PackOptions {
            output,
            inputs,
            algorithm: algo.to_archive(),
            target: target.to_archive(),
            preserve_timestamps,
        }),
        Cmd::Unpack { input, dest } => unpack::unpack(unpack::UnpackOptions { input, dest }),
        Cmd::Inspect { input } => inspect::inspect(inspect::InspectOptions { input }),
        Cmd::ListTargets => {
            println!("8086         shipped (default)");
            println!("286          planned");
            println!("386          planned (phase 3)");
            println!("486          planned (phase 5)");
            println!("pentium      planned (phase 3)");
            println!("pentium-mmx  planned (phase 5)");
            println!("p2           planned (phase 5)");
            println!("p3           planned (phase 5)");
            Ok(())
        }
        Cmd::ListAlgos => {
            println!("stored       shipped (default in phase 1)");
            println!("aplib        planned (phase 2; via apultra)");
            println!("lzsa2        planned (phase 6; fast decompression)");
            println!("lzma         planned (phase 5; best ratio, 386+ only)");
            Ok(())
        }
    }
}
