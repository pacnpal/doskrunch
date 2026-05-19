# Changelog

Notable changes per release. The [Unreleased] section tracks what's on
`main` but not yet tagged.

## [Unreleased]

Nothing yet.

## [v1.0.0] — 2026-05-19

First public release. Cross-platform CLI that produces self-extracting
DOS .EXE archives, with CPU-tier-tuned decompressors hand-ported into
the stub. Runs on the original 8088 IBM PC up through a Pentium III.

### Algorithms

- `aplib` (default, via vendored apultra). Best ratio-vs-stub-size
  tradeoff. Beats `gzip -9` on small files. Decompressor is ~200 bytes
  of asm. Available on every CPU tier.
- `stored`. No compression, baseline fallback. Available on every CPU
  tier.
- `lzma` (via vendored xz-embedded MicroLZMA decoder in compact memory
  model + lzma-rust encoder on the host). Best ratio.
  `--target 386+` only.
- `lzsa2` (via vendored lzsa, with Jim Leonard's 8088 tuning on the
  decoder). Fastest decompression on retro CPUs. Available on every
  CPU tier.

### CPU target tiers

8086 / 286 / 386 / 486 / pentium / pentium-mmx / p2 / p3. Each tier
gets its own per-tier stub blob. The 8086 tier is the default and
covers the original 1981 IBM PC.

The pentium-mmx, p2, and p3 stubs ship an MMX-accelerated aPLib depacker
(8-byte MOVQ block copy when offset and length are both ≥ 8; scalar
`rep movsb` for short or overlapping matches; EMMS on exit).

### Subcommands

- `pack` — build an SFX from input files or directories.
- `unpack` — host-side extraction (no DOS required).
- `inspect` — print archive header + per-file table (name, chunk
  count, sizes, CRC32) and the run-after command when set.
- `list-targets` / `list-algos` — what shipped in the build.

### Flags worth knowing

- `--algo {aplib,stored,lzma,lzsa2}` and
  `--target {8086,286,386,486,pentium,pentium-mmx,p2,p3}`.
- `--chunk-size <bytes>`. Per-algorithm ceilings; default 16 KiB.
- `--run-after "PROG.EXE /args"`. Stores a command at archive
  `run_after_offset`. The host plumbing is shipped; the stub-side
  INT 21h/4Bh EXEC lands in v1.1 (see Known limitations).
- `--preserve-timestamps`. Opt out of the default reproducible-build
  behaviour (zeroed mtimes).

### Reproducible builds

On by default: source mtimes zeroed, file entries sorted by 8.3 name
before serialization, no environment-derived padding. Same input
bytes produce the same output bytes across hosts and runs.

### Distribution

Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows
x86_64. Built via the `release.yml` workflow on a 5-way native
runner matrix. `cargo install --git https://github.com/pacnpal/doskrunch`
works on every supported host.

### Tests + verification

- 87 unit + 14 roundtrip + 3 aplib_roundtrip Rust tests.
- 17 ignored DOSBox-X correctness gates covering single-chunk +
  multi-chunk extraction across all 8 CPU tiers for stored / aplib /
  lzma / lzsa2, plus the 2 MB memsize=2 / timestamps /
  stored-max-chunk gates from earlier phases.

### Known limitations (carried forward)

- **`--run-after` stub-side EXEC is deferred to v1.1.** Host writes
  the flag + offset + command bytes; stub reads the metadata but
  doesn't invoke the command yet. Watcom's `system()` adds ~4.5 KiB
  and pushes the 8086 blob past its 8 KiB hard ceiling; a hand-rolled
  INT 21h/4Bh wrapper is the v1.1 path. The `run_after_offset` is a u16,
  which caps the total compressed archive size at roughly 64 KiB when
  this flag is used. The format is stable, so SFXs packed today will
  work transparently once v1.1 ships.
- **SSE depacker variant for p3 not linked in.** `aplib_depack_sse.asm`
  ships in source but `aplib_p3.bin` uses the MMX depacker. Under
  DOSBox-X 2026.05.02 cputype=pentium_iii the SSE path hangs on
  multi-chunk payloads despite correct-looking NASM encoding. Needs
  validation on a real Pentium III or a different emulator.
- **MMX-vs-pentium aPLib speed gate**, **LZMA-vs-aPLib
  decompression-time gate**, and **Phase 3 386/pentium aPLib speedup
  gate** are documented but not measured. DOSBox-X is too noisy a
  substrate for cycle-accurate comparison; isolating decode time
  needs stub-side INT 1Ah cycle-counter instrumentation that hasn't
  landed yet.

### Vendored dependencies

- `vendor/apultra` (zlib, Emmanuel Marty) — aPLib codec.
- `vendor/xz-embedded` (0BSD, Lasse Collin) — MicroLZMA decoder.
  Carries two `/* doskrunch patch: */` lines fixing a 16-bit C
  portability bug (`1 << 24` and `3U << 30` are undefined in
  Watcom 16-bit, where `int` is 16-bit).
- `vendor/lzsa` (zlib + CC0, Emmanuel Marty with Jim Leonard tuning)
  — LZSA2 codec.

All three are MIT-compatible. See each vendored directory's
`LICENSE` / `COPYING` for the exact text.

[Unreleased]: https://github.com/pacnpal/doskrunch/compare/v1.0.0...HEAD
[v1.0.0]: https://github.com/pacnpal/doskrunch/releases/tag/v1.0.0
