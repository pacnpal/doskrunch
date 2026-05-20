# DOSKrunch

DOSKrunch wraps a pile of files on a modern machine into a single
self-extracting DOS `.EXE`. Copy that one file to a vintage box, run it, and it
unpacks itself. No DOS-side tooling, no separate archiver on the target.

The part that does not exist anywhere else: the decompressor stub is built for
the CPU you are aiming at. Pick a target from the original 8086 up to a Pentium
III and DOSKrunch ports in a depacker that uses what that chip actually has
(32-bit real-mode registers on a 386, `BSWAP` on a 486, MMX on a Pentium MMX,
an SSE match-copy on a Pentium III). Everything still runs in 16-bit real mode.

The CLI binary and crate are named `doskrunch` (lowercase), so the commands
below are typed as `doskrunch ...`. "DOSKrunch" is the project's stylized name.

## What should I use?

For most DOS machines, use the defaults. No flags:

```
doskrunch pack out.exe files/
```

That is aPLib compression on the `8086` target: the best, most compatible
choice. aPLib runs on every x86 CPU from the 1981 8088 up and compresses well
(the decompressor is about 200 bytes), and the `8086` target runs on any DOS
machine. If you do not have a specific reason to choose otherwise, stop here.

Pick something else only for a concrete reason:

- Fastest unpacking on a 4.77 MHz 8088 (original IBM PC or XT):
  `doskrunch pack --algo lzsa2 --target 8086 out.exe files/`
- Smallest possible archive, on a 386 or newer:
  `doskrunch pack --algo lzma --target 386 out.exe files/`
- Input is already compressed (`.zip`, `.jpg`, `.mp3`):
  `doskrunch pack --algo stored out.exe media/`
- Faster unpacking on a known newer CPU: raise `--target` (for example
  `--target 486` or `--target pentium`). It speeds up unpacking only; it does
  not change the archive size.

Rules of thumb:

- An SFX built for a target runs on that CPU and newer, never on older ones. A
  `386`-targeted SFX runs on a 386 and up but not on a real 8086 or 286.
  Targeting a higher tier only makes unpacking faster, never changes the archive
  size or correctness on newer CPUs. When in doubt, target lower; `8086` runs
  everywhere.
- Smaller archives come from the algorithm (`lzma` < `aplib` < `lzsa2` <
  `stored`), not from the target tier.
- `lzma` needs `--target 386` or higher. The CLI refuses `8086` and `286` for
  LZMA with a clear error. `aplib`, `lzsa2`, and `stored` run on every tier.

## Install

DOSKrunch is a single self-contained binary named `doskrunch`. Two ways to get
it.

### Build with Cargo (any platform)

Needs a recent Rust toolchain from <https://rustup.rs>. Same on Linux, macOS,
and Windows:

```
cargo install --git https://github.com/pacnpal/doskrunch --locked
```

Cargo installs `doskrunch` into `~/.cargo/bin` (on Windows,
`%USERPROFILE%\.cargo\bin`), which rustup already put on your `PATH`. Check it:

```
doskrunch --help
```

### Download a prebuilt binary (no Rust needed)

Grab the archive for your platform from the releases page:
<https://github.com/pacnpal/doskrunch/releases>. Builds exist for Linux
(x86_64 and aarch64), macOS (x86_64 and aarch64, pick aarch64 for Apple Silicon
and x86_64 for Intel Macs), and Windows (x86_64).

On Linux:

```
tar xzf doskrunch-*-linux-*.tar.gz
chmod +x doskrunch
sudo mv doskrunch /usr/local/bin/
doskrunch --help
```

On macOS (Gatekeeper quarantines downloaded binaries, so clear it once):

```
tar xzf doskrunch-*-macos-*.tar.gz
chmod +x doskrunch
xattr -d com.apple.quarantine doskrunch 2>/dev/null || true
sudo mv doskrunch /usr/local/bin/
doskrunch --help
```

On Windows, unzip the archive and run it from that folder, or move
`doskrunch.exe` into a folder on your `PATH`:

```
doskrunch.exe --help
```

The command arguments are identical on every platform. Windows users type
`doskrunch.exe` (or just `doskrunch` once it is on the `PATH`).

## Algorithms

- `aplib` (default): best ratio-versus-stub-size tradeoff, beats `gzip -9` on
  small files, decompressor is about 200 bytes. Runs on every tier.
- `lzsa2`: fastest decompression. Pick it when the SFX should feel snappy on a
  4.77 MHz 8088, or when extraction time matters more than archive size. Runs
  on every tier.
- `lzma`: best ratio. Bigger stub (about 17 KiB) and needs the 386's 32-bit
  registers, so it is 386 and up only.
- `stored`: no compression. Useful for already-compressed input or for testing.
  Runs on every tier.

## CPU targets

- `8086` (default): pure 16-bit, runs on the original 1981 IBM PC.
- `286`: adds `PUSHA`/`POPA` and `IMUL imm`. Marginal over 8086 in practice.
- `386`: 32-bit registers in real mode, a big jump for the copy loop.
- `486`: `BSWAP` and better instruction scheduling.
- `pentium`: U/V pipe pairing.
- `pentium-mmx`: MMX-accelerated match copy in the aplib depacker.
- `p2`: Pentium II (P6 out-of-order core) plus the MMX baseline, so a p2
  build needs MMX (it won't run on a Pentium Pro).
- `p3`: SSE-accelerated aPLib match copy.

## Commands

- `pack <output> <inputs...>`: build an SFX. Directory inputs are walked
  recursively (symlinks are skipped). Files extract flat at runtime regardless
  of where they sit in the source tree.
- `unpack <input> -d <dest>`: extract on the host. Does not need DOS or DOSBox.
- `inspect <input>`: print the archive header and the per-file table.

Useful flags:

- `--algo {aplib,stored,lzma,lzsa2}`: algorithm, default `aplib`.
- `--target {8086,286,386,486,pentium,pentium-mmx,p2,p3}`: CPU tier, default
  `8086`.
- `--chunk-size <bytes>`: per-chunk uncompressed size, default 16 KiB. Caps at
  16 KiB for `aplib`, `lzma`, and `lzsa2` (the stub's BSS budget) and 65535 for
  `stored`.
- `--preserve-timestamps`: keep source mtimes instead of the default
  reproducible-build behavior (zeroed timestamps).

## Examples

Pack a directory with the safe defaults, then inspect and host-extract it:

```
doskrunch pack out.exe README.md src/
doskrunch inspect out.exe
doskrunch unpack out.exe -d extracted/
```

Tighter compression for a 386 or better:

```
doskrunch pack --algo lzma --target 386 setup.exe big-payload/
```

Fastest unpacking on a real 4.77 MHz 8088:

```
doskrunch pack --algo lzsa2 --target 8086 fast.exe app/
```

## Links

- Source: <https://github.com/pacnpal/doskrunch>
- Releases: <https://github.com/pacnpal/doskrunch/releases>
- Issues: <https://github.com/pacnpal/doskrunch/issues>
