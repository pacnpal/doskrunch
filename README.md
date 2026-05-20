# DOSKrunch

DOSKrunch is a cross-platform CLI that produces self-extracting DOS .EXE
archives. The output runs on real DOS, from the original 8088 IBM PC up through
a Pentium III, with CPU-tier-tuned decompressors hand-ported into the stub.

(The CLI binary and crate are named `doskrunch` — lowercase — so the commands
below are typed as `doskrunch …`. "DOSKrunch" is the project's stylized name.)

Built because nothing on the market produces CPU-tier-targeted DOS SFXs from a
modern host. The result is a single static binary you `cargo install` once and
keep around for whenever you need to ship something to a vintage box.

## TL;DR — what should I use?

**For most DOS machines, use the defaults — no flags:**

```bash
doskrunch pack out.exe files/
```

That's **aPLib compression on the `8086` target: the best, most compatible
choice.** It runs on *every* DOS machine, from the 1981 IBM PC/8088 right up to
a Pentium III, and still compresses well. If you're not sure, this is the answer.

Only pick something else for a specific reason — fastest unpacking on a 4.77 MHz
8088 (`--algo lzsa2`), the smallest possible file on a 386+ (`--algo lzma --target 386`),
or faster unpacking on a known newer CPU (raise `--target`, e.g. `--target pentium`).
See [Which options should I use?](#which-options-should-i-use) for the full
rundown, and [Install](#install) for per-platform setup.

## Install

DOSKrunch is a single self-contained binary named `doskrunch`. There are two ways to get it.

### Option A — build with Cargo (any platform)

Needs a recent [Rust toolchain](https://rustup.rs) (install via rustup). Works the same on Linux, macOS, and Windows:

```bash
cargo install --git https://github.com/pacnpal/doskrunch --locked
```

Cargo drops the `doskrunch` binary in `~/.cargo/bin` (Windows: `%USERPROFILE%\.cargo\bin`), which rustup already put on your `PATH`. Verify:

```bash
doskrunch --help
```

### Option B — download a prebuilt binary (no Rust needed)

Grab the archive for your platform from the [Releases page](https://github.com/pacnpal/doskrunch/releases) — Linux (x86_64 / aarch64), macOS (x86_64 / aarch64 — pick `aarch64` for Apple Silicon, `x86_64` for Intel Macs), or Windows (x86_64). Then:

**Linux**
```bash
tar xzf doskrunch-*-linux-*.tar.gz       # or unzip, depending on the asset
chmod +x doskrunch
sudo mv doskrunch /usr/local/bin/          # or anywhere on your PATH
doskrunch --help
```

**macOS**
```bash
tar xzf doskrunch-*-macos-*.tar.gz
chmod +x doskrunch
# Gatekeeper quarantines downloaded binaries; clear it once:
xattr -d com.apple.quarantine doskrunch 2>/dev/null || true
sudo mv doskrunch /usr/local/bin/
doskrunch --help
```
(If macOS still blocks it: System Settings → Privacy & Security → "Open Anyway".)

**Windows (PowerShell)**
```powershell
# Unzip the downloaded doskrunch-*-windows-x86_64.zip, then from that folder:
.\doskrunch.exe --help
# To run it from anywhere, move doskrunch.exe into a folder on your PATH
# (e.g. create C:\Tools, add it to PATH, and copy doskrunch.exe there).
```
SmartScreen may warn about an unsigned download — choose "More info → Run anyway".

> No prebuilt assets yet? Until the first tagged release lands, use **Option A** (`cargo install`).

Everywhere below, Windows users type `doskrunch.exe` (or just `doskrunch` once it's on `PATH`); Linux/macOS users type `doskrunch`. The arguments are identical.

## Quick start

```bash
# Defaults: aplib compression, 8086 target (maximum DOS compatibility).
doskrunch pack out.exe README.md src/

# Tighter compression on retro hardware that has a 386 or better.
doskrunch pack --algo lzma --target 386 setup.exe big-payload/

# Fast decompression on a real 4.77 MHz 8088.
doskrunch pack --algo lzsa2 --target 8086 fast.exe app/

# Take a peek at what's inside an SFX without booting DOS.
doskrunch inspect out.exe

# Extract on the host (no DOS required).
doskrunch unpack out.exe -d extracted/
```

## Algorithms

| Algorithm | 8086 / 286 | 386+ | Use case |
|-----------|------------|------|----------|
| `aplib`   | yes        | yes  | Default. Best ratio-vs-stub-size tradeoff. Beats `gzip -9` on small files. Decompressor is ~200 bytes of asm. |
| `stored`  | yes        | yes  | No compression. Useful for already-compressed input or for verifying the chunk plumbing. |
| `lzma`    | no         | yes  | Best ratio. Bigger stub (~17 KiB) and needs the 386's 32-bit registers. |
| `lzsa2`   | yes        | yes  | Fastest decompression. Pick this when you want the SFX to feel snappy on a 4.77 MHz 8088 or when extraction time matters more than archive size. |

## Targets

| Target          | What it picks up from the CPU |
|-----------------|-------------------------------|
| `8086` (default)| Pure 16-bit. Runs on the original 1981 IBM PC. |
| `286`           | `PUSHA` / `POPA` and `IMUL imm`. Marginal over 8086 in practice. |
| `386`           | 32-bit registers in real mode. Big jump for the LZ copy loop. |
| `486`           | `BSWAP`; better instruction scheduling. |
| `pentium`       | U/V pipe pairing. |
| `pentium-mmx`   | MMX-accelerated match copy in the aplib depacker. |
| `p2`            | Pentium Pro / P6 codegen + MMX baseline. |
| `p3`            | SSE-accelerated aPLib match-copy path (MOVUPS 16-byte blocks when match offset and length are both >= 16). |

LZMA requires `--target 386` or higher. The CLI refuses `--algo lzma --target
8086` (or `286`) with a clear error. Everything else works on every tier.

## Which options should I use?

**Best, most compatible choice: the defaults — `aplib` compression on the `8086`
target.** That's just:

```bash
doskrunch pack out.exe files/
```

aPLib runs on every x86 CPU from the 1981 8088 up and gives excellent compression
(a ~200-byte decompressor), and the `8086` target runs on *any* DOS machine. If you
don't have a specific reason to choose otherwise, stop here — this is the right pick.

Reach for a different combination only when you have a concrete need:

| Your situation | Pick | Command |
|----------------|------|---------|
| **Not sure — make it run anywhere** (recommended) | `aplib` + `8086` *(default)* | `doskrunch pack out.exe files/` |
| Original IBM PC/XT (4.77 MHz 8088), want the *fastest* unpacking | `lzsa2` + `8086` | `doskrunch pack --algo lzsa2 --target 8086 out.exe files/` |
| 386/486-era machine, want tighter compression | `aplib` + `386` | `doskrunch pack --algo aplib --target 386 out.exe files/` |
| Pentium-class machine, shipping a large payload, want the *smallest* file | `lzma` + `p2` | `doskrunch pack --algo lzma --target p2 out.exe files/` |
| Input is already compressed (`.zip`, `.jpg`, `.mp3`…) | `stored` | `doskrunch pack --algo stored out.exe media/` |

Rules of thumb:
- An SFX built for a target runs on that CPU **and newer**, never on older ones: a
  `386`-targeted SFX runs on a 386 and up but **not** on a real 8086/286. Targeting a
  higher tier only makes unpacking faster — it doesn't change the archive size (the
  compressed bytes are the same for a given algo regardless of target) or correctness
  on newer CPUs. When in doubt, target lower; `8086` runs everywhere.
- Smaller archives come from the *algorithm* (`lzma` < `aplib` < `lzsa2` < `stored`),
  not the target tier.
- `lzma` needs `--target 386` or higher (the CLI refuses `8086`/`286` with a clear error).
- `aplib`, `lzsa2`, and `stored` run on every tier, `8086` through `p3`.

## Subcommands

- `pack <output> <inputs...>` — build an SFX. Directory inputs are walked
  recursively; symlinks are skipped. Files extract flat at runtime regardless
  of where they live in the source tree.
- `unpack <input> -d <dest>` — host-side extraction. Doesn't need DOS or
  DOSBox-X.
- `inspect <input>` — print the archive header and per-file table.
- `list-targets` / `list-algos` — what's shipped in this build.

Useful flags:

- `--algo {aplib,stored,lzma,lzsa2}` — algorithm. Default `aplib`.
- `--target {8086,286,386,486,pentium,pentium-mmx,p2,p3}` — CPU tier. Default
  `8086`.
- `--chunk-size <bytes>` — per-chunk uncompressed size. Default 16 KiB. Caps:
  16 KiB for aplib / lzma / lzsa2 (stub BSS budget), 65535 for stored.
- `--preserve-timestamps` — opt out of the default reproducible-build behaviour
  (zeroed mtimes).

## Reproducible builds

On by default: source mtimes are zeroed, file entries are sorted
lexicographically by the stored 8.3 name, no environment-derived padding. The
same input bytes produce the same output bytes. Opt back into source mtimes
with `--preserve-timestamps`.

## Build from source

The host CLI is plain Rust:

```bash
cargo build --release
cargo test --workspace
```

The DOS stubs are built inside a pinned Open Watcom v2 Docker image, with
NASM for the asm pieces. The committed `stubs/blobs/*.bin` files are the
output; rebuilding them locally is only needed if you change `stubs/src/` or
the vendored xz-embedded / lzsa / apultra trees.

```bash
docker build -t doskrunch-watcom stubs/
docker run --rm --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" -w /work/stubs doskrunch-watcom make all
```

CI rebuilds the stubs and fails the build if the committed blobs drift.

## Limitations

Phase 5 and 6 deferred a few items that didn't pay for themselves in
measurement:

- **`--run-after` u16 offset ceiling**. `--run-after`
  encodes the command's archive byte offset in a u16, so the
  cumulative archive prefix (25-byte header + all per-file records,
  including the per-chunk compressed data bytes themselves) has to
  fit in 65,535 bytes. The DKCH on-disk layout puts chunk data
  inline with each file's record, so chunk bytes count toward the
  ceiling. In practice this caps the total compressed size of the
  archive at roughly 64 KiB when `--run-after` is in use. A small
  setup payload (config files, a one-off batch script, a tiny .EXE
  to launch) fits comfortably; a multi-megabyte SFX doesn't.
  `pack` bails with a clear "cumulative archive prefix exceeds the
  65535 byte u16 run_after_offset ceiling" error rather than
  silently truncating.
- **MMX-vs-pentium aplib speed gate** (PLAN.md §10). aPLib's bit-at-a-time
  decoder doesn't expose enough vectorizable surface for a measurable 30%
  speedup. Gate **redefined**: the MMX depacker is correct and wired in;
  it provides a 0–5% speedup on payloads with many long, non-overlapping
  matches (offset ≥ 8, length ≥ 8). The 30% "literal-heavy" threshold is
  removed because aPLib literals have no vector-copyable structure (no
  literal-run opcode — each literal requires a separate bit-decode). See
  PLAN.md §10 for the full rationale. The bench-only decode-timing harness
  (`make bench` blobs + `benchmark_tier_decompression` in
  `host/tests/benchmark_tiers.rs`) measures isolated decode time via the
  INT 1Ah BIOS tick counter (8086+) for local measurement, though it does
  not isolate the pentium-mmx-vs-pentium delta specifically.
- **LZMA-vs-aPLib decompression-time gate** (PLAN.md §10). DOSBox-X is a
  noisy substrate for cycle-accurate comparisons. The multi-chunk LZMA gate
  finishes in roughly 3x the aPLib gate's wall-clock; cleanly isolating
  decode time from DOS startup overhead needs stub-side `INT 1Ah` cycle-
  counter instrumentation that hasn't landed yet.

## License

MIT. Vendored dependencies:

- `vendor/apultra` — zlib (Emmanuel Marty)
- `vendor/lzsa` — zlib + CC0 for `src/matchfinder.c` (Emmanuel Marty)
- `vendor/xz-embedded` — 0BSD (Lasse Collin and contributors)

All compatible with MIT for the resulting binary. See each vendored
directory's `LICENSE` / `COPYING` file for the exact text.

## Design and history

PLAN.md is the design spec. It explains why each algorithm got picked, how
the stub fits inside the small-model DS limit, the per-tier size budgets,
and the phased plan that built up to the current shape.

tasks/todo.md tracks per-phase progress, including the deferred items
listed above and the reasoning behind them.
