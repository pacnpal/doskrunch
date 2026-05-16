# doskrunch task list

Phase ordering is strict. No starting phase N+1 until N's verify passes and the user confirms.

## Phase 1: store-only SFX, 8086 tier

- [x] License (MIT), workspace `Cargo.toml`, host crate skeleton.
- [x] CLAUDE.md.
- [ ] Archive container encode/decode in Rust (DKCH magic, full PLAN.md §8 layout, `chunk_count=1`).
- [ ] CRC32 + roundtrip unit tests.
- [ ] `pack` / `unpack` / `inspect` / `list-targets` / `list-algos` clap subcommands.
- [ ] `--reproducible` default-on, `--preserve-timestamps` opt-out.
- [ ] Placeholder stub blob committed (zeroed) so host crate builds before Watcom stub lands.
- [ ] Watcom C stub: `stubs/src/stub.c`, `dos.h`, `Makefile`, `Dockerfile`.
- [ ] CI: `build-stubs.yml` builds + commits `stubs/blobs/stored_8086.bin`.
- [ ] CI: `test.yml` runs `cargo test` and DOSBox-X headless integration.
- [ ] `tests/fixtures/` payloads + Rust integration test for host roundtrip.
- [ ] DOSBox-X integration test (CI-only, `cpu_type=8086`).
- [ ] cargo-fuzz target for the archive parser.

**Phase 1 verify**

- [ ] `cargo test` green.
- [ ] `doskrunch pack` → `doskrunch unpack` byte-identical against fixtures.
- [ ] DOSBox-X headless extraction byte-identical against fixtures.

## Phase 2: aPLib (8086)

Not started.

## Phase 3: 386 + pentium tiers

Not started.

## Phase 4: chunked extraction, large payloads

Not started.

## Phase 5: LZMA + remaining tiers

Not started.

## Phase 6: LZSA2 + polish + release

Not started.
