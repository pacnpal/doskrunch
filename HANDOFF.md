# doskrunch handoff — Phase 1 partial

This document is a handoff for the next Claude Code session. The previous
session stopped because the sandbox has no Docker daemon, blocking the
local Watcom build and DOSBox-X integration validation. Everything that
can be done without those is committed.

PLAN.md and CLAUDE.md are the authoritative docs. Karpathy guidelines apply.

## What's done (committed on branch `claude/build-doskrunch-cli-Kgkxd`)

1. License: **MIT** (`LICENSE` at repo root).
2. Workspace scaffold: `Cargo.toml`, `host/Cargo.toml`, `.gitignore`.
3. `CLAUDE.md` (build/test commands, CPU tier table, stub size budgets,
   algorithm priority, reproducibility default).
4. `tasks/todo.md` (per-phase checkable list).
5. Rust host crate `host/`:
   - `src/archive.rs` — full PLAN.md §8 DKCH container (header + per-file
     records + trailer with back-pointer + CRC32). Roundtrip + CRC-flip +
     bad-magic + multi-chunk + empty-archive unit tests (12 tests).
   - `src/fat_time.rs` — Unix-secs → FAT dos_date/dos_time (Hinnant
     civil-from-days; clamps to 1980..=2107).
   - `src/name83.rs` — 8.3 mangling + `~N` collision rename.
   - `src/pack.rs` — `pack` subcommand. Reproducible-by-default (sorted
     entries, zero timestamps); `--preserve-timestamps` opts out. Rejects
     `--algo lzma --target {8086,286}`. Rejects directory inputs (recursive
     walk lands in Phase 4 with chunking).
   - `src/unpack.rs` — host-side `unpack` (no DOS needed). Verifies file
     CRC32 per entry.
   - `src/inspect.rs` — `inspect` subcommand.
   - `src/stubs.rs` — `include_bytes!` of `stubs/blobs/stored_8086.bin`.
   - `src/main.rs` — clap CLI: `pack`, `unpack`, `inspect`, `list-targets`,
     `list-algos`.
   - `tests/roundtrip.rs` — host-only end-to-end: pack → unpack → diff,
     reproducibility (two packs byte-identical), inspect smoke. **3 integration
     tests pass on top of 20 unit tests = 23 total green.**
6. Watcom stub **source** (compile NOT yet verified):
   - `stubs/src/stub.c` — Phase 1 stored / 8086 stub. Uses Watcom's
     `_dos_open` / `_dos_creat` / `_dos_read` / `_dos_write` / `_dos_close`
     / `_dos_setftime` plus `lseek`. Small model, 16 KB scratch buffer.
     Reads its own argv[0], finds DKTR trailer at EOF-8, seeks to DKCH
     header, walks per-file records, writes each file via INT 21h. Skips
     the on-archive header CRC check (host validates on pack) to save
     ~150 bytes of stub code.
   - `stubs/src/dos.h` — typedefs (`u8`, `u16`, `u32`, `i16`, `i32`) +
     `<dos.h>` include. No custom wrappers; the Watcom built-ins do the job.
   - `stubs/Makefile` — GNU make driver for the Linux Docker image.
     Builds `stubs/blobs/stored_8086.bin`, fails build if size > 8 KB
     (PLAN.md §6 hard ceiling).
   - `stubs/Dockerfile` — `FROM volkertb/debian-open-watcom:latest`,
     adds `make`.
7. Test fixtures: `tests/fixtures/{hello.txt, numbers.txt, random.bin, empty.bin}`.
   Deterministic content; no random bytes.
8. Placeholder stub blob: `stubs/blobs/stored_8086.bin` is a minimal
   MZ header (matches the magic check in `pack`) plus zero padding to
   256 bytes. Not a runnable .EXE; the host roundtrip tests don't
   execute it. Replace with the Docker-built blob to get a real SFX.

## What's left to make Phase 1's verify pass

In order. Each step ends with a verify gate.

### A. Build the real Watcom stub blob

Run on a host with Docker available:

```bash
docker build -t doskrunch-watcom stubs/
docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
ls -la stubs/blobs/stored_8086.bin
```

Expected: a real DOS .EXE (MZ header) under 8 KB (target 4 KB).
**Likely follow-ups** because `stub.c` has not been compile-tested:

- Watcom may want `<unistd.h>` for `lseek` or not have `SEEK_SET`/`SEEK_END`
  exposed — adjust includes to `<io.h>` if needed (already included).
- Watcom v2's `_dos_setftime` signature is
  `unsigned _dos_setftime(int handle, unsigned date, unsigned time);` —
  the existing call site treats failure as best-effort; if the symbol is
  named differently in the v2 headers (`_dos_setfileinfo`?) just call
  INT 21h 0x5701 directly via `int86()`.
- The `_dos_write(1, ...)` calls to stdout work in real DOS but `wcl`
  may strip them under `-os` if it inlines them oddly. Verify by running
  in DOSBox-X (next step) and watching for missing per-file echo lines.
- If size exceeds 4 KB target, the easiest wins are (1) drop the per-file
  echo `puts2(namebuf)`, (2) compact the `die` strings into a single
  table indexed by error code.

Verify gate: blob exists, < 8 KB hard ceiling.

### B. Commit the real blob

Replace `stubs/blobs/stored_8086.bin` with the Watcom output and commit.
This keeps host builds reproducible without the Watcom toolchain
(CLAUDE.md "Stub blobs" section spells this out).

### C. CI workflow `build-stubs.yml`

Runs on changes to `stubs/src/**` or `stubs/Makefile`/`Dockerfile`. Steps:

1. `docker build -t doskrunch-watcom stubs/`
2. `docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all`
3. Compare `stubs/blobs/stored_8086.bin` to the committed copy; fail
   if it drifts (so PR authors are reminded to commit the rebuild).
4. Upload the blob as an artifact.

### D. CI workflow `test.yml`

Two jobs running in parallel:

1. **host-tests**: matrix on `ubuntu-latest`, `macos-latest`, `windows-latest`.
   `cargo test --workspace`. Caches `~/.cargo` and `target/`.
2. **dosbox-x-integration**: `ubuntu-latest`, installs `dosbox-x` via apt
   (Ubuntu 24.04 has it). Runs the integration test described in (E).

### E. DOSBox-X headless integration test

Add `tests/integration/phase1_8086.rs` (or a Bash script under
`tests/integration/run_dosbox.sh` driven from a `#[ignore]`'d Rust test).
Workflow inside the test:

1. `cargo run -- pack out.exe tests/fixtures/hello.txt tests/fixtures/numbers.txt tests/fixtures/random.bin tests/fixtures/empty.bin`
2. Generate a temporary `dosbox.conf` with `cpu_type=8086`, `memsize=4`,
   `[autoexec]` that `mount c <tmpdir>; c:; out.exe; exit`.
3. Run `dosbox-x -conf <conf> -exit -nogui` (headless).
4. Diff the extracted files in the mounted dir against the fixtures.
   They must be byte-identical (uppercase 8.3 names: `HELLO.TXT`, `NUMBERS.TXT`,
   `RANDOM.BIN`, `EMPTY.BIN`).
5. Mark the test `#[ignore]` so local `cargo test` skips it when DOSBox-X
   isn't installed; CI runs `cargo test -- --ignored --include-ignored`.

This is the gate that proves the stub actually works on a real DOS-like
CPU. If it fails, expect to iterate on stub.c (most likely culprits:
argv[0] not absolute → re-derive the self path from the PSP environment
via INT 21h 0x62; or `_dos_creat` attribute byte wrong).

### F. cargo-fuzz target

Phase 1 mandate. Add `host/fuzz/` via `cargo fuzz init` and a single
target that calls `archive::Archive::read` on the fuzz input. Add a
nightly CI job running for ~60s on every PR.

### G. Final Phase 1 verification

Per the original prompt:

- [ ] `cargo test` green (already ✅ for the 23 we have; will grow with E + F).
- [ ] `doskrunch pack` → `doskrunch unpack` byte-identical against the
      4-file fixture set (already ✅).
- [ ] DOSBox-X headless extraction byte-identical against the fixtures
      with `cpu_type=8086` (blocked on A–E).

When all three pass on a clean CI run, ask the user to confirm Phase 1 is
done before starting Phase 2 (apultra vendor + 16-bit aPLib decoder).

## Reminders for the next session

- Branch: `claude/build-doskrunch-cli-Kgkxd`. Don't push elsewhere.
- Karpathy: no speculative abstractions. The current stub deliberately
  skips the on-archive header CRC check (host validates) and uses 16 KB
  scratch (worst case is one chunk-worth, which is u16-bounded → fine).
- Reproducibility is **default-on**. Phase 1 entries are sorted by stored
  8.3 name and have `timestamp=0`. The `flags::REPRODUCIBLE` bit is set
  in the archive header.
- Pre-commit hooks: none configured yet. Don't add any. Do NOT use
  `--no-verify` even though there's nothing to bypass.
- Reuse the existing PR for follow-up commits; don't open a second one.

## Resume prompt (paste this into the next session)

> Resume doskrunch Phase 1 on branch `claude/build-doskrunch-cli-Kgkxd`.
> Read `HANDOFF.md` first — it documents what's done and what's left.
> Read `PLAN.md` and `CLAUDE.md` for the spec and Karpathy guidelines.
> Then continue from section "What's left to make Phase 1's verify pass"
> step A. You have Docker available; the previous session did not, which
> is why the Watcom stub source is committed but never compiled.
>
> Order: build the Watcom stub (step A), iterate on `stubs/src/stub.c`
> if it fails to compile or run, commit the real blob (B), wire CI for
> stubs + tests (C, D), add the DOSBox-X integration test (E), add the
> cargo-fuzz target (F). Then ask me to confirm Phase 1 is complete
> before moving on to Phase 2 (aPLib via apultra).
>
> Don't speculate, don't refactor adjacent code, don't push to any
> branch besides `claude/build-doskrunch-cli-Kgkxd`, and verify each
> step before declaring it done.
