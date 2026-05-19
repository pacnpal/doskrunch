# Changelog

All notable changes to doskrunch are documented in this file.

## [Unreleased] — v1.1

### Added

- **Stub-side `--run-after` EXEC** (closes issue v1.1: stub-side INT 21h/4Bh
  EXEC). The DOS stubs now invoke the program named by `--run-after` after
  extraction using a hand-rolled INT 21h/4Bh wrapper (`stubs/src/exec_dos.asm`,
  ~60 bytes, `cpu 8086`-clean). The wrapper saves SS:SP and DS into CS-relative
  words before the call and restores them afterward (DOS EXEC destroys the
  caller's stack-segment registers). Both `stub.c` (small model, all aplib/lzsa2
  tiers) and `stub_lzma.c` (compact model, LZMA tiers) split the command string
  at the first space to separate the program name (DS:DX for 4Bh) from the
  counted argument tail, fill a 14-byte EXEC parameter block (env_seg=0 to
  inherit, cmdline offset/segment, FCBs from PSP:5Ch/6Ch), close the SFX file
  handle before EXEC, and exit 0 on return. The child's errorlevel is not
  propagated (out-of-scope per the v1.1 design note).

- **DOSBox-X gate `dosbox_run_after`** (`host/tests/dosbox_run_after.rs`).
  Three `#[ignore]`-gated tests:
  - `run_after_aplib_8086` — aplib stub, no-args EXEC, 8086 CPU.
  - `run_after_aplib_386` — aplib stub, EXEC with arguments (`"DONE.COM /S"`),
    386 CPU; exercises the space-split + counted command line path.
  - `run_after_lzma_386` — LZMA stub (compact model), 386 CPU.

### Changed

- All 14 stub blobs rebuilt. Blob size growth from the EXEC primitive:
  aplib 8086/286: +336 bytes (6746 → 7082, hard ceiling 8192 ✓);
  aplib 386/486: +400 bytes; aplib pentium/pentium-mmx/p2/p3: +448 bytes;
  LZMA all tiers: +400 bytes. All within per-tier hard ceilings.

- README Limitations section: removed the "stub-side execution deferred to
  v1.1" entry for `--run-after`. The u16 offset ceiling note is retained.

- `tasks/todo.md`: stub-side EXEC item marked complete.
