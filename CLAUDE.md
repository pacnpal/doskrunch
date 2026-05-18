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

Default invocation today (Phase 4): `doskrunch pack out.exe files-or-dirs...` → `--algo aplib --target 8086`. Directory inputs are walked recursively (symlinks skipped); files extract flat at runtime regardless of the source tree's shape. The 8086 stub dispatches at runtime on the archive's algorithm byte, so `--algo stored` keeps working against the same blob. Phase 3 also ships `--target 386` (wcc -3 + 32-bit-register aPLib depacker from `aplib_depack_32.asm`) and `--target pentium` (wcc -5 + speed-optimized fast-variant depacker from `aplib_depack_p5.asm`); both blobs dispatch on the archive's algorithm byte the same way the 8086 blob does. `--chunk-size <bytes>` (Phase 4) overrides the default 16 KiB chunk; the ceiling is 16 KiB for aplib (stub BSS budget) and `u16::MAX` for stored.

## Reproducible builds

On by default: timestamps zeroed, file entries sorted lexicographically by stored 8.3 name, no environment-derived padding. Opt back into source mtimes with `--preserve-timestamps`.

## DOSBox-X integration tests

Headless DOSBox-X gates live in `host/tests/dosbox_*.rs`, each `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked. Run them locally with:

```bash
SDL_VIDEODRIVER=dummy cargo test --workspace -- --ignored
```

Eight DOSBox-X correctness gates ship (six from Phase 3, two added in Phase 4):

- `dosbox_8086` — Phase 1 default-algo smoke test. Packs the small fixture set with no `--algo` flag, so it exercises whichever algorithm the host currently defaults to (Phase 2+ → `aplib`). Runs under `cputype=8086`.
- `dosbox_aplib_8086`, `dosbox_aplib_386`, `dosbox_aplib_pentium` — explicit `--algo aplib` packs of the small fixture set at the matching `--target`, run under the matching `cputype=`. Each fixture fits in a single 16 KiB aPLib chunk.
- `dosbox_aplib_large` — packs a 500 KiB synthetic mixed-content payload with `--algo aplib` at each of the three tiers and runs the SFX under the matching `cputype=`. At 500 KiB the payload spans ~32 chunks per file, so this gate exercises the stub's per-chunk decode loop end-to-end on a real-mode CPU emulation.
- `dosbox_stored_all_tiers` — explicit `--algo stored` packs at each of the three tiers under the matching `cputype=`. The stub's stored branch (`algo == 0`, streaming `copy_bytes` through `g_buf`) is a different runtime path from the aplib branch; this gate verifies it works under Watcom's `-3` / `-5` C codegen for the new tiers.
- `dosbox_2mb_memsize2` (Phase 4) — 2 MiB compressible synthetic payload packed at each of the three tiers and run under DOSBox-X with `memsize=2`, `xms=false`, `ems=false`, `umb=false`. The 2 MB payload is never resident in conventional RAM — the chunked decoder streams it 16 KiB at a time — so byte-identical extraction at memsize=2 is the gate that proves the stub's BSS budget actually bounds working memory.
- `dosbox_timestamps` (Phase 4) — two cases. (1) A pinned source mtime (deliberately not "now", set via `filetime::set_file_mtime`) round-trips through pack → DOSBox-X → host `fs::metadata`; the archive's timestamp field is checked exactly, the extracted file's mtime is checked with ±24 h slack to absorb the host's local-time offset (DOS interprets FAT dates as local time). (2) Pre-1980 source mtime is clamped to the FAT epoch endpoint by `fat_time::unix_to_fat`; the extracted file's mtime should land near 1980-01-01.

Shared test plumbing (`WaitError`, `wait_with_timeout`, `locate_case_insensitive`, `repo_root`) lives in `host/tests/common/mod.rs`; each `dosbox_*.rs` file still writes its own `dosbox.conf` inline because the per-test config knobs differ (memsize, xms/ems flags, cputype, autoexec).

Each gate asserts byte-identical extraction. The 500 KiB tier benchmark (`benchmark_tiers`) is also `#[ignore]`-gated but additionally requires the `DOSKRUNCH_RUN_BENCHMARK=1` env var (so the CI `--ignored` run doesn't silently rewrite the committed `tests/benchmarks/results.md`); run it locally with:

```bash
DOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy cargo test --test benchmark_tiers -- --ignored --nocapture
```

## Phase status

Track per-phase progress in `tasks/todo.md`. Phase N+1 starts only after Phase N verification passes and the user confirms.

## Don't

- Don't write to repositories other than `pacnpal/doskrunch`.
- Don't bypass pre-commit hooks or signing.
- Don't add features outside the current phase's scope.
