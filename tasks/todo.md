# doskrunch task list

Phase ordering is strict. No starting phase N+1 until N's verify passes and the user confirms.

## Phase 1: store-only SFX, 8086 tier

- [x] License (MIT), workspace `Cargo.toml`, host crate skeleton.
- [x] CLAUDE.md.
- [x] Archive container encode/decode in Rust (DKCH magic, full PLAN.md §8 layout, `chunk_count=1`).
- [x] CRC32 + roundtrip unit tests.
- [x] `pack` / `unpack` / `inspect` / `list-targets` / `list-algos` clap subcommands.
- [x] `--reproducible` default-on, `--preserve-timestamps` opt-out.
- [x] Watcom C stub: `stubs/src/stub.c`, `stubs/Makefile`, `stubs/Dockerfile`.
- [x] Watcom Docker image switched to upstream `ow-snapshot.tar.xz` on
      `debian:bookworm-slim` (the previously-referenced
      `volkertb/debian-open-watcom` image is gone from Docker Hub).
- [x] Real `stubs/blobs/stored_8086.bin` committed (6 KB, MZ-format).
- [x] CI: `build-stubs.yml` builds the Watcom image and fails if the
      committed blob drifts from a rebuild.
- [x] CI: `test.yml` runs `cargo test` on Linux/macOS/Windows, the
      DOSBox-X headless integration test on Ubuntu, and a 60-second
      libFuzzer run on the archive parser under nightly.
- [x] `tests/fixtures/` payloads + Rust integration test for host roundtrip.
- [x] DOSBox-X integration test (`host/tests/dosbox_8086.rs`, `#[ignore]`-gated).
- [x] cargo-fuzz target for the archive parser (`host/fuzz/`, `fuzz_archive_read`).

**Phase 1 verify**

- [x] `cargo test --workspace` green (35 unit + 4 integration + 1 ignored).
- [x] `doskrunch pack` → `doskrunch unpack` byte-identical against fixtures.
- [x] DOSBox-X headless extraction byte-identical against fixtures
      (`cpu_type=8086`, `memsize=4`).

## Phase 2: aPLib (8086)

- [x] Vendor `vendor/apultra` via `git subtree` (zlib license; output
      fully compatible with original Joergen Ibsen aPLib format).
- [x] Compile apultra C sources (compressor + decompressor) into a
      static lib via `cc-rs` from `host/build.rs`.
- [x] Rust binding `host/src/compress/aplib.rs` over
      `apultra_compress` / `apultra_decompress`.
- [x] `build_aplib_entry` in `host/src/archive.rs` — 16 KiB
      uncompressed chunk cap so worst-case expansion fits in u16.
- [x] Algorithm dispatch in `host/src/pack.rs` and `host/src/unpack.rs`.
- [x] 16-bit NASM depacker `stubs/src/aplib_depack_16.asm`, ported
      from `vendor/apultra/asm/8088/aplib_8088_small.S`.
- [x] Runtime dispatch in `stubs/src/stub.c` on the archive's
      algorithm byte (0=stored streaming, 1=aplib via depacker).
- [x] `stubs/Dockerfile` installs `nasm`.
- [x] `stubs/Makefile` produces `stubs/blobs/aplib_8086.bin` (≤8 KB
      hard ceiling).
- [x] Flip `--algo` default from `stored` to `aplib` in
      `host/src/main.rs`.
- [x] New tests: aplib roundtrip unit tests in `archive::tests`,
      aplib compressor unit tests in `compress::aplib::tests` (incl.
      beats-gzip-9 assertions), `host/tests/aplib_roundtrip.rs`
      host-side end-to-end.
- [x] `host/tests/dosbox_aplib_8086.rs` — `#[ignore]`-gated DOSBox-X
      integration test parallel to `dosbox_8086.rs`.
- [x] Commit the Watcom-built `stubs/blobs/aplib_8086.bin`
      (6384 bytes, under the 8 KB hard ceiling).

**Phase 2 verify**

- [x] `cargo test --workspace` green (45 unit + 7 integration + 2
      ignored DOSBox-X gates).
- [x] `cargo run -- pack o.exe tests/fixtures/*` with no flags
      produces a strictly smaller `.EXE` than the same call with
      `--algo stored`.
- [ ] DOSBox-X headless extraction with `--algo aplib` byte-identical
      against fixtures (`cpu_type=8086`, `memsize=4`). Verified once
      CI's `dosbox-x-integration` job runs against the new blob.

## Phase 3: 386 + pentium tiers

- [x] Port apultra's `asm/x86/aplib_x86_small.asm` to
      `stubs/src/aplib_depack_32.asm` (bits 16, cpu 386, Watcom small-
      model regparm ABI). Replaces the upstream `call .init_get_bit /
      pop ebp` size trick (broken under 16-bit push width) and
      `push 3 / pop ebx` with the explicit `mov bp,…` /
      `mov ebx, 3` patterns from the 8086 port.
- [x] Port apultra's `asm/x86/aplib_x86_fast.asm` to
      `stubs/src/aplib_depack_p5.asm` (bits 16, cpu pentium, macro-
      inlined `apl_get_bit`). No manual U/V pipe scheduling in this
      revision — Karpathy: measure before scheduling.
- [x] Single-source `stubs/src/stub.c` — all three depacker `.obj`
      files export the same `aplib_depack` symbol with the same C
      ABI, so the Makefile picks the `.obj` per tier without a
      `-DALGO_DEPACK_*` toggle.
- [x] `stubs/Makefile` builds three blobs: `aplib_8086.bin` (wcc -0
      + aplib_depack_16.asm, hard ceiling 8 KB), `aplib_386.bin`
      (wcc -3 + aplib_depack_32.asm, hard ceiling 10 KB),
      `aplib_pentium.bin` (wcc -5 + aplib_depack_p5.asm, hard
      ceiling 12 KB).
- [x] Three blobs committed under `stubs/blobs/`. Sizes:
      8086 = 6400 B, 386 = 6416 B, pentium = 6464 B — all well
      under their hard ceilings. The 8086 blob is byte-identical
      to the Phase 2 committed copy when rebuilt from the same
      Watcom snapshot (reproducibility preserved).
- [x] `host/src/stubs.rs` embeds the two new blobs via
      `include_bytes!` and routes `(Stored|Aplib, I386|Pentium)` →
      the matching blob.
- [x] `host/src/main.rs::list-targets` flips 386 and pentium from
      "planned (phase 3)" to "shipped".
- [x] `host/tests/dosbox_aplib_386.rs` and
      `host/tests/dosbox_aplib_pentium.rs` — `#[ignore]`-gated
      DOSBox-X integration tests parallel to the 8086 version.
- [x] `tests/benchmarks/results.md` populated by
      `host/tests/benchmark_tiers.rs` (also `#[ignore]`-gated).

**Phase 3 verify**

- [x] `cargo test --workspace` green (45 unit + 7 integration + 4
      ignored DOSBox-X gates + 1 ignored benchmark gate).
- [x] `SDL_VIDEODRIVER=dummy cargo test -- --ignored` extracts
      byte-identical fixtures under `cputype=8086`, `cputype=386`,
      and `cputype=pentium` (four DOSBox-X gates pass locally).
- [x] Stub blob sizes within hard ceilings for every tier.
- [ ] PLAN.md §10 Phase 3 Verify: "386 is 2-4x faster than 8086,
      pentium is 5-10x faster" speedup gate. **Not met.**
      `tests/benchmarks/results.md` currently shows 1.00× / 1.00× /
      1.10× under DOSBox-X with `cycles=auto`. What we have data
      for: correctness. The DOSBox-X correctness gates
      (`dosbox_8086`, `dosbox_aplib_8086`, `dosbox_aplib_386`,
      `dosbox_aplib_pentium`, and the multi-chunk
      `dosbox_aplib_large`) all extract byte-identical at every
      tier, so we know the depackers produce correct output. Where
      the speedup went is currently a hypothesis, not a
      measurement: DOSBox-X auto-cycles likely tunes per-cputype
      throughput in a way that flattens relative-CPU comparison,
      and DOS startup + INT 21h file I/O likely dominate the 2 s
      wall-clock. The harness measures end-to-end SFX runtime, not
      isolated depacker time, so we can't currently rule out a
      genuinely slow port. Confirming or refuting that needs
      isolated depacker timing (stub-side INT 1Ah cycle counter
      around the `aplib_depack` call) and/or real-iron
      measurement (86Box / a real 386 or Pentium box). Phase 3
      ships the ports and the correctness gates; this perf-gate
      row is left explicitly unchecked so the user can direct
      (accept the limitation as-is, instrument the stub, gather
      real-iron data, or block Phase 4 until one of those lands).
      The decision to flag rather than re-tune the asm follows
      CLAUDE.md's Karpathy guideline ("push back on speculative
      work") and the working brief, not a literal directive in
      PLAN.md.

## Phase 4: chunked extraction, large payloads

Not started.

## Phase 5: LZMA + remaining tiers

Not started.

## Phase 6: LZSA2 + polish + release
