use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use doskrunch::archive::{APLIB_CHUNK_INPUT, LZMA_CHUNK_INPUT, LZSA2_CHUNK_INPUT};
use doskrunch::{archive, inspect, pack, unpack};

#[derive(Parser)]
#[command(
    name = "doskrunch",
    version,
    about = "Build self-extracting DOS .EXE archives.",
    long_about = "Build self-extracting DOS .EXE archives from files and directories.

The output is one .EXE that unpacks itself on real DOS, from the 1981 IBM PC up
to a Pentium III. Take the defaults (aPLib compression on the 8086 target) for
the most compatible result; that combination runs on any DOS machine. Reach for
a different --algo or --target only for a specific need (speed, size, or a known
newer CPU).

Quick start:
  doskrunch pack out.exe files/        pack a directory (recursive) with the defaults

Run `doskrunch <COMMAND> --help` for that command's full options.",
    after_help = "Examples:
  doskrunch pack out.exe README.md src/                     most compatible (aPLib + 8086)
  doskrunch pack --algo lzsa2 --target 8086 fast.exe app/   fastest unpack on a 4.77 MHz 8088
  doskrunch pack --algo lzma --target 386 small.exe big/    smallest archive (386 or newer)
  doskrunch pack --no-recurse out.exe topdir/               top-level files only, no subdirs
  doskrunch pack --max-depth 2 out.exe topdir/              descend at most 2 directory levels
  doskrunch inspect out.exe                                 peek inside without booting DOS
  doskrunch unpack out.exe -d extracted/                    extract on the host"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a self-extracting DOS .EXE from the given input files.
    ///
    /// Directory inputs are walked recursively by default; only regular
    /// files are included. Use --no-recurse to pack only each directory's
    /// immediate files, or --max-depth N to limit how deep the walk goes
    /// (find(1) style: 1 = immediate files only). Symlinks are skipped
    /// (whether named as a top-level input or found during the walk).
    /// Files extract into the current directory at runtime regardless of
    /// their position in the source tree (flat extraction); two files
    /// with the same basename across different subdirectories get
    /// FAT-style `~N` dedupe suffixes. The output path itself is excluded
    /// from the walk so re-running `pack dir/OUT.EXE dir/` doesn't pack a
    /// previous SFX into the new one.
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
        /// Don't descend into subdirectories of directory inputs: pack
        /// only each directory's immediate files. Equivalent to
        /// `--max-depth 1`. Conflicts with --max-depth.
        #[arg(long, conflicts_with = "max_depth")]
        no_recurse: bool,
        /// Maximum directory depth to walk for directory inputs, find(1)
        /// style: 1 = a directory's immediate files only (no
        /// subdirectories), 2 = one level of subdirectories, and so on.
        /// Default: unlimited (full recursion). Must be >= 1. Conflicts
        /// with --no-recurse.
        #[arg(long, value_name = "N")]
        max_depth: Option<usize>,
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
    // No subcommand: print the full help (long_about + commands +
    // examples) to stdout and exit 0, instead of clap's terse
    // missing-subcommand error (exit 2). `--help` still works as usual.
    let Some(cmd) = cli.cmd else {
        Cli::command().print_long_help()?;
        return Ok(());
    };
    match cmd {
        Cmd::Pack {
            output,
            inputs,
            algo,
            target,
            preserve_timestamps,
            chunk_size,
            run_after,
            no_recurse,
            max_depth,
        } => {
            // --no-recurse is shorthand for --max-depth 1 (the two
            // conflict at the CLI layer, so at most one is set here).
            let max_depth = if no_recurse { Some(1) } else { max_depth };
            if max_depth == Some(0) {
                bail!("--max-depth must be >= 1 (got 0); 1 = a directory's immediate files only");
            }
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
                max_depth,
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
