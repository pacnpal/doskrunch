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
- [x] PLAN.md §10 Phase 3 Verify perf gate measured with isolated
      decode timing. `host/tests/benchmark_tiers.rs` now records
      `INT 1Ah` ticks around `aplib_depack` (plus wall-clock) and
      regenerates `tests/benchmarks/results.md` with the per-tier
      ratios for 8086 / 386 / pentium. Current result:
      386/8086 = 1.01×, pentium/8086 = 1.31× (**gate not met** on
      DOSBox-X `cycles=auto`). The gate expectation remains
      documented in PLAN.md with an explicit measured verdict.

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
  Deferred in Phase 4; landed later via `host/tests/benchmark_tiers.rs`
  + `stubs/src/stub.c` DKPERF sidecar timing.


## Phase 5: LZMA + remaining tiers

Phase 5 ships in five logical pieces on `claude/doskrunch-phase-5`,
landing together as one PR. The aplib side fills out the five
remaining CPU tiers (286, 486, pentium-mmx, p2, p3); the LZMA side
vendors xz-embedded, wires it both host-side and stub-side, and
ships per-tier LZMA blobs at 386 through p3.

- [x] aplib_286.bin (wcc -2 + aplib_depack_16.asm), aplib_486.bin
      (wcc -4 + aplib_depack_32.asm), aplib_pentium-mmx.bin,
      aplib_p2.bin, aplib_p3.bin (all wcc -5/-6 + the existing p5
      depacker initially). Five new per-tier blobs reusing the
      existing depackers.
- [x] aplib_depack_mmx.asm — MMX 8-byte MOVQ block copy on the match
      hot path when offset >= 8 and length >= 8; scalar `rep movsb`
      fallback for shorter or overlapping matches; EMMS on exit. Wired
      into pentium-mmx, p2, p3. Blob size grew 6464 -> 6512 bytes.
- [x] aplib_depack_sse.asm — MOVUPS 16-byte block-copy variant; on
      disk but NOT linked in. Under DOSBox-X cputype=pentium_iii the
      MOVUPS-based copy hangs on multi-chunk payloads despite
      correct-looking NASM encoding. Documented as deferred in
      stubs/blobs/README.md and stubs/Makefile; p3 falls back to
      the MMX depacker. Follow-up needs a real Pentium III box or
      a different emulator to prove the suspicion that this is a
      DOSBox-X SSE emulation gap rather than a depacker bug.
- [x] Vendor xz-embedded ae63ae3 under `vendor/xz-embedded/` via
      `git subtree add --squash`. License 0BSD (public-domain
      equivalent, MIT-compatible). README + CLAUDE.md document the
      pinned SHA.
- [x] host/build.rs compiles xz-embedded's MicroLZMA decoder
      (xz_crc32.c + xz_dec_lzma2.c with -DXZ_DEC_MICROLZMA) into a
      separate `xz_embedded` static lib. xz_dec_stream.c (the .xz
      container parser) intentionally NOT included — MicroLZMA's
      one-byte-per-chunk framing is enough for the doskrunch archive,
      and the .xz container would add ~40 bytes per chunk for
      already-redundant length/CRC fields.
- [x] host/src/compress/lzma.rs — encoder via lzma-rust 0.1
      (Apache-2.0). LZMAWriter::new(out, opts, use_header=false,
      use_end_marker=false, ...) produces a raw LZMA1 stream; we
      replace its first byte (always 0x00 from range coder init)
      with the bitwise-negated MicroLZMA properties byte and ship
      that. Decoder via the vendored xz-embedded FFI. Eight unit
      tests cover empty / short / multi-chunk round-trips, size-
      mismatch detection, deterministic encoding, and the PLAN.md
      §10 "LZMA beats aPLib > 100 KB" gate (200 KiB LCG-derived
      mixed-content payload). LZMA_DICT_SIZE pinned at 16 KiB; the
      stream doesn't carry the dict size in-band so the producer
      and consumer agree out of band.
- [x] archive::build_lzma_entry parallel to build_aplib_entry /
      build_stored_entry. LZMA_CHUNK_INPUT = 16 KiB (matches aPLib
      so the chunk count is algorithm-independent at the same
      --chunk-size). LZMA_MAX_COMPRESSED_CHUNK = 17 KiB (LZMA's
      worst-case expansion plus the 1-byte MicroLZMA prefix).
      Roundtrip tests in archive::tests.
- [x] pack.rs: Algorithm::Lzma flows into build_lzma_entry on
      target 386+; bails at pack() entry with a clear error on
      target 8086/286.
- [x] unpack.rs: Algorithm::Lzma decodes via
      compress::lzma::decompress (the same xz-embedded FFI the
      stub uses). Host-side roundtrip working at all 6 LZMA tiers.
- [x] main.rs / list-algos: LZMA flipped from "planned (phase 5)"
      to "shipped". CLI --chunk-size validation has an LZMA arm.
- [x] roundtrip::lzma_rejected_on_8086_and_286_at_the_cli_layer
      confirms the CLI surfaces the "requires --target 386+"
      message on both rejected tiers.
- [x] stubs/src/stub_lzma.c — LZMA-only stub.c (algo == 3 only;
      anything else dies loudly). XZ_SINGLE mode so the output
      buffer IS the LZMA2 dictionary (no separate dict allocation).
      Compact memory model (-mc) so xz-embedded's `uint8_t *`
      becomes `uint8_t __far *` and the ~32 KiB decoder state can
      live in its own data segment — small model's malloc caps a
      single allocation at 32 KiB and `struct xz_dec_microlzma` is
      bigger than that, so the obvious -ms approach hits a wall.
- [x] Two xz-embedded portability patches for 16-bit C, both
      `/* doskrunch patch: */`-marked in the vendored source:
      RC_TOP_VALUE = `(1 << 24)` → `((uint32_t)1 << 24)` and the
      two `> (3U << 30)` bounds checks → `> ((uint32_t)3 << 30)`.
      In 16-bit Watcom, `1` and `3U` are 16-bit `int` / `unsigned
      int`, so a 24- or 30-bit shift is undefined behavior; Watcom
      truncates to 0 which made rc_normalize never refill and
      xz_dec_microlzma_alloc reject every nonzero dict_size. Both
      are upstream-able portability fixes.
- [x] stubs/Makefile: six per-tier LZMA build rules
      (lzma_{386,486,pentium,pentium-mmx,p2,p3}.bin), compiled
      under wcc -mc with `-DXZ_DEC_MICROLZMA` and the
      vendor/xz-embedded include paths. Each blob lands at 16,840
      bytes (target ≤18,432 / hard ceiling 24,576 for 386..p2;
      ≤20,480 / 28,672 for p3).
- [x] .github/workflows/build-stubs.yml mounts the project ROOT
      (not just stubs/) so the Makefile can reach
      `../vendor/xz-embedded/` from /work/stubs. The paths-trigger
      now includes `vendor/xz-embedded/**` so an xz subtree update
      forces a stub rebuild.
- [x] host/src/stubs.rs: dispatch table grew six LZMA entries.
- [x] host/tests/dosbox_lzma_all_tiers.rs — fixture-set extraction
      at every LZMA tier under headless DOSBox-X (~10 s locally).
- [x] host/tests/dosbox_lzma_large.rs — 500 KiB multi-chunk payload
      at every LZMA tier (~170 s locally). Catches the
      second-chunk-corruption class of bug the single-chunk gate
      misses.
- [x] dosbox_aplib_large.rs and dosbox_stored_all_tiers.rs grown
      from 3 tiers to all 8.

**Phase 5 verify**

- [x] `cargo test --workspace` green: 73 unit + 3 aplib_roundtrip
      + 12 roundtrip integration tests (was 50+/3/10 in Phase 4;
      the +23 unit tests are LZMA codec coverage + archive LZMA
      builder + roundtrip CLI LZMA rejection).
- [x] `SDL_VIDEODRIVER=dummy cargo test --workspace -- --ignored`
      green across all 14 ignored DOSBox-X gates locally under
      dosbox-x 2026.05.02 (was 11 in Phase 4): the 11 existing
      gates plus dosbox_aplib_new_tiers, dosbox_lzma_all_tiers,
      and dosbox_lzma_large. benchmark_tiers is also `#[ignore]`-d
      but additionally requires `DOSKRUNCH_RUN_BENCHMARK=1`, so
      it fast-skips under the plain `--ignored` invocation and
      isn't counted here. The 8-tier dosbox_stored_all_tiers and
      8-tier dosbox_aplib_large gates each test 8 tier-runs, so
      total tier-runs covered is roughly 30 across the suite.
- [x] Stub blob count: 8 aplib + 6 LZMA = 14 blobs (was 3 after
      Phase 3, 3 after Phase 4). All within hard ceilings.
- [x] PLAN.md §10 "LZMA beats aPLib > 100 KB" gate met by the
      host unit test on a 200 KiB realistic payload.
- [x] PLAN.md §4 "LZMA requires 386+" enforced at three layers:
      CLI (chunk-size validation), pack() (algorithm gate), and
      stub_for() (no LZMA blob for 8086 / 286 targets).

**Not done in Phase 5 (deferred deliberately)**

- SSE depacker variant for p3. aplib_depack_sse.asm exists in
  stubs/src/ with a 16-byte MOVUPS block-copy path; the Makefile
  doesn't link it. Under DOSBox-X 2026.05.02 cputype=pentium_iii
  the SSE path hangs on multi-chunk payloads despite a correct-
  looking NASM encoding. Validating on a real Pentium III box or
  a different emulator is the next step. p3 uses the MMX depacker
  instead, which is also a wcc -6 win on the surrounding C
  housekeeping.
- MMX speed gate from PLAN.md §10 ("pentium-mmx aplib at least
  30% faster than pentium aplib on a literal-heavy payload"). The
  MMX depacker is wired up and correct, but a measurable 30%
  speedup is unlikely to materialize: aPLib literals are emitted
  one byte at a time gated on a bit-decode (no literal-run opcode
  the MMX path could accelerate), so the vectorizable surface is
  the rarer "long match with offset >= 8" case. Same Karpathy
  "push back on speculative work" framing the Phase 3 perf-gate
  row uses; left open as a measurement question rather than a
  code-quality question.
- LZMA-vs-aPLib decompression-time gate from PLAN.md §10 ("LZMA
  decompression on 386 tier completes within 10x the aPLib
  decompression time on the same payload"). Same noisy-substrate
  concern as the Phase 3 perf gate; the multi-chunk LZMA gate
  finishes in ~170 s (aPLib finishes in ~50 s) across 6 tiers,
  so the per-tier ratio is in the right ballpark, but cleanly
  isolating LZMA decode time from DOS startup + INT 21h overhead
  needs stub-side INT 1Ah cycle-counter instrumentation that the
  Phase 3 row defers.
- Phase 3 perf-gate row (386 / pentium aplib speedup): now measured
  with isolated decode timing and documented as "not met" in
  `tests/benchmarks/results.md`.
- Default --chunk-size bump to 32 KiB. Same memory-model concern
  Phase 4 documented; LZMA tightens it further because the LZMA
  stub already uses compact model and the chunk size also bounds
  the dict.

## Phase 6: LZSA2 + polish + release

Phase 6 is the v1 ship phase: LZSA2 as the third algorithm (universal,
8086+), polish on the `inspect` subcommand, README rewrite, and a
GitHub Releases workflow for five host platforms. The run-after-extract
piece is wired host-side; the stub-side INT 21h/4Bh EXEC lands once a
Docker rebuild path is clear (the local containerd cache hit an I/O
error mid-phase; CI's build-stubs.yml is the fallback).

- [x] Vendor lzsa 15ee2dfe under `vendor/lzsa/` via `git subtree add
      --squash`. License is zlib (with `src/matchfinder.c` under CC0),
      both MIT-compatible.
- [x] host/build.rs compiles lzsa as a third cc-rs static lib
      alongside apultra and xz-embedded. Files mirror lzsa's Makefile
      LIBOBJS minus the CLI front-end (`src/lzsa.c`) and the stream
      I/O wrappers; doskrunch only uses the in-memory API
      (`shrink_inmem.c` / `expand_inmem.c`) with
      `LZSA_FLAG_RAW_BLOCK`.
- [x] host/src/compress/lzsa2.rs — Rust binding over two narrow
      `extern "C"` calls: `lzsa_compress_inmem` (raw block, format
      v2, min match 2, favor ratio) and `lzsa_decompress_inmem` (raw
      block, format v2). Six unit tests: empty / short / multi-byte
      / zero / repetitive round-trips, size-mismatch detection,
      deterministic encoding.
- [x] archive::build_lzsa2_entry parallel to build_aplib_entry /
      build_lzma_entry. `LZSA2_CHUNK_INPUT = 16 KiB` matches aPLib so
      the chunk count stays algorithm-independent at the same
      `--chunk-size`; `LZSA2_MAX_COMPRESSED_CHUNK = 17 KiB` covers
      LZSA2's worst-case expansion plus the small block header. Four
      archive roundtrip tests in `archive::tests`.
- [x] pack.rs: `Algorithm::Lzsa2` flows through to build_lzsa2_entry
      on every tier (no target restrictions). chunk_size validation
      grows an LZSA2 arm bounded by `LZSA2_CHUNK_INPUT`.
- [x] unpack.rs: `Algorithm::Lzsa2` decodes via
      `compress::lzsa2::decompress`. Host pack-then-unpack parity
      tests cover stored / aplib / lzma / lzsa2 end-to-end.
- [x] main.rs: list-algos flips `lzsa2` from "planned (phase 6)" to
      "shipped". Pack help-text triple-slash comment describes all
      four algorithms honestly. The "deferred algorithm takes
      precedence" CLI arm collapses since every algorithm is shipped;
      the test that gated it (`deferred_algorithm_takes_precedence_…`)
      becomes `lzsa2_chunk_size_above_ceiling_is_rejected` to keep
      the chunk-size validation path covered.
- [x] stubs/src/lzsa2_depack_16.asm — port of lzsa's
      `asm/8088/decompress_small_v2.S` (Marty's 8088 small-size
      decoder + Trixter tuning). cpu 8086 / bits 16. Same Watcom
      small-model regparm ABI as aplib_depack; save/restore ES + BP;
      labels prefixed `l2_` for OMF visibility.
- [x] stubs/src/lzsa2_depack_32.asm — port of lzsa's
      `asm/x86/decompress_small_v2.asm` (32-bit size-opt decoder).
      bits 16 + cpu 386 with NASM's auto-emitted 0x66 / 0x67
      prefixes; mirrors aplib_depack_32.asm's adaptations.
- [x] stubs/src/stub.c grows an algo == 2 branch alongside stored
      (algo 0) and aplib (algo 1). LZSA2 reuses `g_src` and `g_buf`
      so no additional BSS is consumed — LZSA2's per-chunk
      compressed cap (17 KiB) fits in `APLIB_SRC_SIZE` (18464 B) and
      its uncompressed cap (16 KiB) equals `BUF_SIZE`. The algo gate
      moves from `algo > 1` to `algo > 2`; LZMA (algo == 3) still
      lives in the separate stub_lzma.c blob.
- [x] stubs/Makefile builds the two new NASM .obj files and links
      the matching one (16 for 8086/286, 32 for 386+) into every
      aplib_<tier>.bin. Stub blob count stays at 14.
- [x] inspect.rs: per-file `chunk` column. Catches multi-chunk
      decoder regressions and visualizes how a payload was split.
- [x] README.md (new file). Quick-start, algorithm × target matrix,
      recommended defaults, subcommand reference, build-from-source
      steps, and a Limitations section listing the three Phase 5 / 6
      deferred items. No duplication of PLAN.md content.
- [x] .github/workflows/release.yml — 5-way native matrix building
      doskrunch for linux-x86_64 / linux-aarch64 / macos-x86_64 /
      macos-aarch64 / windows-x86_64. Triggered on `v*` tag push;
      workflow_dispatch dry-runs the matrix without publishing.
      Per-job `permissions: contents: read`, scoped `contents: write`
      on the publish step. softprops/action-gh-release pinned by SHA.
- [x] host/tests/dosbox_lzsa2_all_tiers.rs — fixture-set extraction
      at every shipped tier (all 8). Parallel to
      dosbox_aplib_new_tiers.rs / dosbox_lzma_all_tiers.rs.
- [x] host/tests/dosbox_lzsa2_large.rs — 500 KiB multi-chunk LZSA2
      payload at every tier. Parallel to dosbox_aplib_large.rs and
      dosbox_lzma_large.rs.

**Pending blocker (CI-resolvable)**

- Stub blob rebuild. The local containerd cache hit an I/O error
  mid-phase (`docker images` fails with a corrupt-blob message).
  CI's `build-stubs.yml` mounts the project root + invokes `make
  all`, which rebuilds the 14 blobs and uploads the artifact. The
  drift check will then surface the new blob bytes to be committed.
  The two LZSA2 DOSBox-X gates above are written but won't pass
  against the older committed blobs — they need the new blobs to
  land first.

**Run-after-extract: host-side shipped, stub-side deferred to v1.1**

- [x] CLI: `--run-after <command>` on `doskrunch pack`. Accepts
      a NUL-terminated printable-ASCII string up to 127 bytes (the
      stub's `RUN_AFTER_BUF` cap minus the NUL). Validates via
      `Archive::set_run_after`, which flips `flags::RUN_AFTER` and
      stores the command bytes for serialization.
- [x] Archive: `Archive::write` computes `run_after_offset` (header
      size + per-file records) and emits the command bytes after the
      file records. `Archive::read` parses them back and rejects
      inconsistent flag/offset combinations. Four roundtrip tests:
      offset round-trip, set_run_after validation, no-run-after
      keeps offset zero, flag-vs-offset consistency at read time.
- [x] CLI test coverage: roundtrip pack -> inspect verifies the
      command + flag both appear; invalid-input test confirms the
      CLI bails on non-printable bytes in the command.
- [x] inspect: prints the run-after command line + archive offset
      when the flag is set.
- [x] Stub.c / stub_lzma.c: read the run_after_offset from the
      header and reserve `RUN_AFTER_BUF = 128` bytes of BSS for the
      command. The fields are read-and-ignore (`(void)flags;` etc.)
      pending the v1.1 EXEC wiring.
- [ ] **Deferred: stub-side INT 21h/4Bh EXEC.** Watcom's
      `system(g_run_after)` is the obvious wrapper but adds
      ~4.5 KiB of COMMAND.COM lookup + spawn machinery, which
      pushes the 8086 blob from 6746 to 11234 bytes (past its 8 KiB
      hard ceiling). The cheaper path is a hand-rolled inline-asm
      wrapper around INT 21h/4Bh (parameter block + counted command
      line + child FCB / SS:SP handling) but needs careful real-DOS
      edge-case testing for error paths, errorlevel propagation, and
      child-PSP setup. Left for v1.1; the archive format is stable
      so a v1.1 stub can pick up SFXs packed today.

**Phase 6 verify**

- [x] `cargo test --workspace` green: 87 unit + 14 roundtrip + 3
      aplib_roundtrip (was 73/12/3 at the end of Phase 5). The +14
      unit tests are the LZSA2 codec (6), archive LZSA2 builder (4),
      and archive run-after roundtrip / validation (4). The +2
      roundtrip tests are the LZSA2 chunk-size CLI gate and the
      run-after-via-CLI roundtrip.
- [ ] `SDL_VIDEODRIVER=dummy cargo test --workspace -- --ignored`
      green across the now 17 ignored DOSBox-X gates (was 15 in
      Phase 5: the 15 existing gates plus dosbox_lzsa2_all_tiers and
      dosbox_lzsa2_large). Pending stub blob rebuild.
- [x] PLAN.md §4 universality: LZSA2 packs at every tier from 8086
      through p3. Host-side roundtrip confirms; DOSBox-X gates
      pending blob rebuild.
- [ ] PLAN.md §10 Phase 6 "LZSA2 faster than aPLib on 8086" speed
      gate. Pending blob rebuild; the same noisy-DOSBox-X-substrate
      concern Phase 3's perf row and Phase 5's MMX gate raise
      probably applies — if the gate doesn't measure cleanly, it
      stays open with a documented rationale.
- [x] Stub blob count stays at 14 (8 unified aplib/stored/lzsa2 +
      6 LZMA-only). LZSA2 doesn't fragment into its own blobs the
      way LZMA had to.
- [x] PLAN.md §11 v1 done-criteria items addressed: README is real,
      reproducible builds, single static binary, release workflow
      produces all five host-platform binaries. Run-after-extract
      and the LZMA-vs-aPLib speed gate are the remaining v1 items
      pending the Docker-blocker and follow-up measurement work.

**Not done in Phase 6 (deferred deliberately)**

- SSE depacker variant for p3 (carry-forward from Phase 5).
- MMX-vs-pentium aplib speed gate (carry-forward from Phase 5).
- LZMA-vs-aPLib decompression-time gate (carry-forward from Phase 5).
- Phase 3 perf-gate row: measured and documented (gate not met under
  DOSBox-X `cycles=auto`).
