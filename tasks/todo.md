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

Not started.

## Phase 3: 386 + pentium tiers

Not started.

## Phase 4: chunked extraction, large payloads

Not started.

## Phase 5: LZMA + remaining tiers

Not started.

## Phase 6: LZSA2 + polish + release
