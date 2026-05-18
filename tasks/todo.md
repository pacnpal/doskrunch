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

Not started.

## Phase 4: chunked extraction, large payloads

Not started.

## Phase 5: LZMA + remaining tiers

Not started.

## Phase 6: LZSA2 + polish + release
