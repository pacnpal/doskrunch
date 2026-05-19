# doskrunch

A cross-platform CLI that produces self-extracting DOS .EXE archives. The
output runs on real DOS, from the original 8088 IBM PC up through a Pentium III,
with CPU-tier-tuned decompressors hand-ported into the stub.

Built because nothing on the market produces CPU-tier-targeted DOS SFXs from a
modern host. The result is a single static binary you `cargo install` once and
keep around for whenever you need to ship something to a vintage box.

## Install

```bash
cargo install --git https://github.com/pacnpal/doskrunch
```

Or grab a prebuilt binary from the GitHub Releases page (Linux x86_64,
Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64).

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
| `p3`            | Same as p2 for now; SSE-accelerated copy paths are deferred (see Limitations). |

LZMA requires `--target 386` or higher. The CLI refuses `--algo lzma --target
8086` (or `286`) with a clear error. Everything else works on every tier.

## Recommended defaults

- "I don't know what to pick" → `doskrunch pack out.exe files/` (aplib on 8086).
- "Original IBM PC, decompression speed matters" → `--algo lzsa2 --target 8086`.
- "Modern retro hardware, 386 onward" → `--algo aplib --target 386`.
- "Late 90s machine, shipping a big payload" → `--algo lzma --target p2`.

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

- **`--run-after` stub-side execution**. The host-side plumbing is
  shipped: `doskrunch pack --run-after "MY.EXE /Q" out.exe ...`
  validates the command, sets the archive's `RUN_AFTER` flag, writes
  the NUL-terminated command bytes into the archive at
  `run_after_offset`, and `doskrunch inspect` prints both. The DOS
  stub reads the metadata but does NOT invoke the command yet. The
  obvious wrapper, Watcom's `system()`, pulls in ~4.5 KiB of
  COMMAND.COM lookup + spawn machinery and pushes the 8086 blob past
  its 8 KiB hard ceiling. The cheaper path is a hand-rolled inline-
  asm INT 21h/4Bh wrapper, which needs careful edge-case testing on
  real DOS for FCB / SS:SP / errorlevel behavior. Deferred to v1.1
  so v1 stays inside its stub-size budgets. The archive format is
  stable, so a v1.1 stub can pick up existing run-after-tagged SFXs
  without re-packing.

  One format-level constraint worth flagging now: `--run-after`
  encodes the command's archive byte offset in a u16, so the
  cumulative archive prefix (25-byte header + sum of per-file
  records, NOT counting the run-after command itself) has to fit
  in 65,535 bytes. In practice that's roughly 16 KiB of file-record
  framing — chunk count, names, CRC32s — independent of how much
  the chunk data weighs. Packing a single big file with `--run-after`
  always fits; packing thousands of small files might not. The
  archive header CRC catches a header-side overflow at parse time;
  `pack` itself bails with a clear "cumulative archive prefix
  exceeds the 65535 byte u16 run_after_offset ceiling" error.
- **SSE depacker variant for p3**. `stubs/src/aplib_depack_sse.asm` ships in
  the source tree with a `MOVUPS` 16-byte block copy, but isn't linked into
  `aplib_p3.bin`. Under DOSBox-X 2026.05.02 with `cputype=pentium_iii` the
  SSE path hangs on multi-chunk payloads despite a correct-looking encoding.
  Validating on a real Pentium III box or a different emulator is the next
  step. p3 currently uses the MMX depacker, which still wins on the
  surrounding C housekeeping via wcc -6 codegen.
- **MMX-vs-pentium aplib speed gate** (PLAN.md §10). aPLib's bit-at-a-time
  decoder doesn't expose enough vectorizable surface for a measurable 30%
  speedup on the literal-heavy payloads the gate cares about. Documented as
  a measurement question, not a code-quality question.
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
