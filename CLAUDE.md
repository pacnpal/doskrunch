# doskrunch

Cross-platform CLI that builds self-extracting DOS .EXE/.COM files from arbitrary inputs. CPU-tier-optimized decompressors from 8086 through Pentium III; all output stays in 16-bit real mode.

PLAN.md is the authoritative design doc. Karpathy guidelines apply: simplicity first, surgical changes, every step has a verify criterion. Push back on speculative work.

## Layout

- `host/` Rust CLI (`doskrunch` binary).
- `stubs/src/` Watcom C + NASM stub sources, per (algorithm, CPU tier).
- `stubs/blobs/` prebuilt stub binaries, committed for reproducible host builds. Embedded via `include_bytes!`.
- `vendor/` third-party C sources (apultra, lzsa, xz-embedded) added via `git subtree` in later phases.
- `tests/{fixtures,integration,benchmarks,fuzz}/` test assets.

## Host build & test

```bash
cargo build --release
cargo test
cargo run --bin doskrunch -- pack out.exe file1 file2
cargo run --bin doskrunch -- unpack out.exe -d outdir
cargo run --bin doskrunch -- inspect out.exe
```

## Stub build

Stubs build inside the Open Watcom v2 Docker image. The Makefile is GNU
make (the Linux image already has `make` installed):

```bash
docker build -t doskrunch-watcom stubs/
docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
```

Watcom CPU flags per tier:

| Tier         | Watcom flag | Notes |
|--------------|-------------|-------|
| 8086 (default) | `-0`      | Pure 16-bit, no 286+ instructions. |
| 286          | `-2`        | PUSHA/POPA, IMUL imm. |
| 386          | `-3`        | 32-bit registers in real mode via 0x66 prefix. |
| 486          | `-4`        | BSWAP, single-cycle most. |
| pentium      | `-5`        | U/V pairing; ASM scheduled by hand. |
| pentium-mmx  | `-5` + MMX  | MMX copy paths. |
| p2           | `-6`        | P6 OoO. |
| p3           | `-6` + SSE  | SSE copy paths. |

Each stub variant emits `stubs/blobs/<algo>_<tier>.bin`, which is a complete DOS .EXE (MZ header + load image). The Rust host appends the doskrunch archive + DKTR trailer directly to this blob — no MZ regeneration.

## Stub size budget

| Tier         | Target | Hard ceiling |
|--------------|--------|--------------|
| 8086 / 286   | 4 KB   | 8 KB         |
| 386 / 486    | 6 KB   | 10 KB        |
| pentium / pentium-mmx / p2 | 8 KB | 12 KB |
| p3           | 10 KB  | 14 KB        |

Build fails if a blob exceeds the hard ceiling.

## Algorithm priority

1. **aPLib** (default, via apultra). Best ratio-vs-stub-size tradeoff. 8086+.
2. **LZSA2** optional, fast-decompression mode for 4.77 MHz machines. 8086+.
3. **LZMA** optional, best ratio, **386+ only**. Host rejects `--algo lzma --target 8086|286`.
4. **stored** always available, no compression. Phase 1 baseline.

Default invocation today (Phase 1): `doskrunch pack out.exe files...` → `--algo stored --target 8086`. The default `--algo` flips to `aplib` once Phase 2 lands.

## Reproducible builds

On by default: timestamps zeroed, file entries sorted lexicographically by stored 8.3 name, no environment-derived padding. Opt back into source mtimes with `--preserve-timestamps`.

## DOSBox-X integration tests

CI uses headless DOSBox-X with `cpu_type=` set per tier. See `tests/integration/`. Each test pins a `cpu_type` (`8086`, `386`, `pentium_mmx`) and a `memsize`. Locally, run `cargo test --test integration -- --ignored` if DOSBox-X is installed.

## Phase status

Track per-phase progress in `tasks/todo.md`. Phase N+1 starts only after Phase N verification passes and the user confirms.

## Don't

- Don't write to repositories other than `pacnpal/doskrunch`.
- Don't bypass pre-commit hooks or signing.
- Don't add features outside the current phase's scope.
