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
- [x] `host/tests/dosbox_aplib_large.rs` — multi-chunk DOSBox-X
      correctness gate: 500 KiB synthetic payload extracted
      byte-identical at every tier under the matching `cputype=`.
- [x] `host/tests/dosbox_stored_all_tiers.rs` — `--algo stored`
      DOSBox-X coverage at each shipped tier. Closes the gap where
      the stub's stored runtime branch (`algo == 0`, no depacker)
      had no DOSBox-X coverage on the new wcc -3 / -5 builds; the
      original Phase 1 `dosbox_8086.rs` smoke test stopped
      covering stored after Phase 2 flipped the host's `--algo`
      default to aplib.
- [x] `tests/benchmarks/results.md` populated by
      `host/tests/benchmark_tiers.rs` (also `#[ignore]`-gated).

**Phase 3 verify**

- [x] `cargo test --workspace` green (45 unit + 7 integration + 4
      ignored DOSBox-X gates + 1 ignored benchmark gate).
- [x] `SDL_VIDEODRIVER=dummy cargo test -- --ignored` extracts
      byte-identical fixtures under `cputype=8086`, `cputype=386`,
      and `cputype=pentium` (six DOSBox-X gates pass locally:
      `dosbox_8086`, `dosbox_aplib_{8086,386,pentium}`,
      `dosbox_aplib_large`, `dosbox_stored_all_tiers`).
- [x] Stub blob sizes within hard ceilings for every tier.
- [ ] PLAN.md §10 Phase 3 Verify: "386 is 2-4x faster than 8086,
      pentium is 5-10x faster" speedup gate. **Not met.**
      `tests/benchmarks/results.md` currently shows 1.00× / 1.00× /
      1.10× under DOSBox-X with `cycles=auto`. What we have data
      for: correctness. The six DOSBox-X correctness gates
      (`dosbox_8086`, `dosbox_aplib_8086`, `dosbox_aplib_386`,
      `dosbox_aplib_pentium`, the multi-chunk `dosbox_aplib_large`,
      and the per-tier `dosbox_stored_all_tiers`) all extract
      byte-identical at every tier, so we know both algorithm
      paths produce correct output on every shipped tier. Where
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

The chunked decoder (`stubs/src/stub.c::main` per-chunk loop) and
the chunked encoder shipped in Phase 2/3: `build_aplib_entry` split
input at `APLIB_CHUNK_INPUT = 16 KiB`, and `build_stored_entry` split
at the per-chunk u16 ceiling (`u16::MAX`). Phase 4's `--chunk-size`
flag exposes both splitters as a user-tunable parameter (default
16384, capped per-algorithm: aplib ≤ 16384, stored ≤ u16::MAX). FAT
timestamp restoration and 8.3 mangling-with-warning shipped in Phase
1/2. Phase 4 fills the user-facing input surface and adds the
verify-gate tests PLAN.md §10 specifies.

- [x] Directory walking in `host/src/pack.rs` — recursive walk via
      `expand_inputs` + `walk_dir`. Symlinks skipped whether named as
      a top-level input or found during the walk (no follow). The
      output path is excluded from the walk so `pack dir/OUT.EXE dir/`
      doesn't pack a previous SFX into the new one. Each `read_dir`
      result sorted per directory so the walk itself is reproducible
      across hosts; pack's downstream "sort by mangled 8.3" pass is
      what guarantees the on-disk byte sequence is identical across
      runs. `read_dir` iteration errors bubble up rather than being
      silently dropped — a partial-walk would silently produce an
      incomplete SFX otherwise.
- [x] `--chunk-size <bytes>` CLI flag. Default 16384 (= stub BSS
      budget); validated at the CLI layer to `1..=16384` for aplib,
      `1..=u16::MAX` for stored. CLI validation is gated on
      shipped algorithms, so `--algo lzma --chunk-size 99999` returns
      the "lzma lands in phase 5" bail from `pack()` rather than a
      chunk-size error against a placeholder ceiling. Stub unchanged —
      the on-disk archive records per-chunk sizes that the stub reads
      back as-is. Because pack reads each input fully into memory
      before encoding, the value controls archive layout (chunk count,
      framing overhead) and the transient encode buffer, NOT peak host
      RAM. 32 KiB default explicitly **not** taken in Phase 4 — see
      the "subtle issues" note in the Phase 4 brief; bumping default
      requires either a memory-model change in the stub or DOS-side
      heap allocation, neither of which the verify gate asks for.
- [x] Test helpers consolidated: `WaitError`, `wait_with_timeout`,
      `locate_case_insensitive`, `repo_root` moved to
      `host/tests/common/mod.rs`. Each `dosbox_*.rs` keeps its own
      `dosbox.conf` template and timeout inline (varies per test).
      Crosses the 6+ files threshold called out in the Phase 3 PR
      review: 6 existing dosbox_*.rs + 1 benchmark + 3 new (2 MB,
      timestamps, stored-max-chunk) = 10 callers.
- [x] `host/tests/dosbox_2mb_memsize2.rs` — `#[ignore]`-gated PLAN.md
      §10 Phase 4 Verify gate. 2 MiB compressible synthetic payload
      packed at each tier; DOSBox-X run with `memsize=2`,
      `xms=false`, `ems=false`, `umb=false`. Asserts byte-identical
      extraction. Confirms the chunked decoder keeps the SFX working
      set bounded by the stub's small-model BSS (~35 KiB) — payload
      is never resident in conventional RAM, the stub streams it
      chunk-by-chunk.
- [x] `host/tests/dosbox_timestamps.rs` — `#[ignore]`-gated PLAN.md
      §10 Phase 4 Verify gate. Two cases: (1) pinned source mtime
      (2024-05-16 12:34:56 UTC, on a FAT 2s boundary) round-trips
      through pack → DOSBox-X → host `fs::metadata`, truncated to 2s.
      (2) Pre-1980 mtime gets clamped to the 1980-01-01 FAT epoch
      endpoint — defends the `fat_time::unix_to_fat` clamp end-to-end
      so a regression that removes it would fail this gate. Source
      mtime is set via `filetime::set_file_mtime` to a fixed value
      rather than wall-clock-now, so the test is stable across reruns
      and across CI host clocks.
- [x] `host/tests/dosbox_stored_max_chunk.rs` — `#[ignore]`-gated
      DOSBox-X gate. 200 KiB payload packed with `--algo stored
      --chunk-size 65535 --target 8086`, run under `cputype=8086`.
      Forces multiple iterations of the stub's `copy_bytes` loop per
      chunk (chunk size 65535 > BUF_SIZE 16384). The aplib gates
      exercise a different stub path (`g_src` + `aplib_depack` +
      `g_buf`), so this is the only end-to-end coverage of the
      stored multi-iteration `copy_bytes` branch.
- [x] Host-side roundtrip tests in `host/tests/roundtrip.rs` for the
      new flag and walker:
      `pack_walks_directory_recursively`,
      `directory_pack_is_deterministic_across_two_invocations`,
      `chunk_size_flag_respected_end_to_end`,
      `chunk_size_above_stub_budget_for_aplib_is_rejected`,
      `chunk_size_above_u16_for_stored_is_rejected`,
      `stored_max_chunk_size_roundtrips_via_host_unpack`.
- [x] CLI help (`host/src/main.rs::Cmd::Pack`) documents directories
      are walked recursively, files extract flat (no subdir recreation
      on DOS), and `--chunk-size` defaults to 16 KiB. The "directories
      not yet supported" placeholder is gone.

**Phase 4 verify**

- [x] `cargo test --workspace` green (50+ unit + 13 integration:
      10 roundtrip + 3 aplib_roundtrip; ignored DOSBox-X gates
      remain gated).
- [x] `SDL_VIDEODRIVER=dummy cargo test --workspace -- --ignored`
      extracts byte-identical fixtures and payloads under
      `cputype=8086`, `cputype=386`, `cputype=pentium` across the
      original six Phase 3 gates plus the three new Phase 4 gates
      (2 MB memsize=2 + timestamps + stored-max-chunk). All 11
      ignored gates pass locally under dosbox-x 2026.05.02. CI's
      `dosbox-x-integration` job runs the same set on Ubuntu 24.04.
- [x] `cargo run -- pack out.exe some/dir/` walks the directory,
      packs every regular file under it in deterministically-sorted
      order, byte-identical across reruns.
- [x] Stub blob sizes unchanged from Phase 3 (the stub is untouched in
      Phase 4 — `--chunk-size` is a host-side knob only; the stub
      reads per-chunk `usize`/`csize` from the archive header
      verbatim).

**Not done in Phase 4 (deferred deliberately)**

- Default chunk size bump to 32 KiB. PLAN.md asks for it but the
  small-model DS budget (BSS ~35 KB at 16 KiB; ~52 KB at 32 KiB)
  doesn't have the headroom without either compact memory model or
  DOS-heap allocation. Today `--chunk-size` lets stored-mode users
  pick a value up to u16::MAX (the default 16384 is below stored's
  ceiling), and lets either algorithm pick a smaller chunk for
  archive-layout tuning; aplib's ceiling IS the default, so an
  aplib-default user can't tune upward without the stub changes
  above. Defaulting up is a focused follow-up that requires the
  memory-model / DOS-heap decision first.
- Subdirectory recreation in the stub. PLAN.md §8 hints at it for
  directory mode, but Phase 4 Verify doesn't require it. Flat
  extraction is the simpler shipping choice and is documented in
  the CLI help. Phase 6 polish can revisit.
- INT 1Ah cycle-counter instrumentation for the Phase 3 perf gate.
  Not bundled into Phase 4 per the working brief; the perf-gate row
  above stays open across phases until the user picks a direction.


## Phase 5: LZMA + remaining tiers

Not started.

## Phase 6: LZSA2 + polish + release
