Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` (`.github/workflows/build-stubs.yml`)
and fails if the committed copy drifts, so the host crate stays buildable
without the Watcom toolchain installed locally.

Phase 5 ships eight aplib blobs (Phase 3 shipped three). All use Open
Watcom v2 (`wcc -bt=dos -ms -os` + `wlink system dos`) and NASM 2.x
(`-f obj`). Each dispatches at runtime on the archive's algorithm byte
(0=stored streaming copy, 1=aPLib via the linked depacker), so the
host returns the same blob for both `--algo stored` and `--algo aplib`
on the requested `--target`.

| Blob | wcc flag | aPLib depacker | Size budget |
|------|----------|----------------|-------------|
| `aplib_8086.bin`        | `-0` | `aplib_depack_16.asm` (145 B 8088 port) | ≤4 KB target / 8 KB hard |
| `aplib_286.bin`         | `-2` | `aplib_depack_16.asm` (depacker is `cpu 8086`, a strict 286 subset) | ≤4 KB target / 8 KB hard |
| `aplib_386.bin`         | `-3` | `aplib_depack_32.asm` (203 B 32-bit size-opt port) | ≤6 KB target / 10 KB hard |
| `aplib_486.bin`         | `-4` | `aplib_depack_32.asm` (depacker is `cpu 386`, runs on every 32-bit CPU) | ≤6 KB target / 10 KB hard |
| `aplib_pentium.bin`     | `-5` | `aplib_depack_p5.asm` (252 B 32-bit speed-opt port, no manual U/V scheduling) | ≤8 KB target / 12 KB hard |
| `aplib_pentium-mmx.bin` | `-5` | `aplib_depack_mmx.asm` (MMX 8-byte MOVQ block copy when offset >= 8 and length >= 8; scalar `rep movsb` otherwise; EMMS on exit) | ≤8 KB target / 12 KB hard |
| `aplib_p2.bin`          | `-6` | `aplib_depack_mmx.asm` (same depacker as pentium-mmx; -6 codegen for the surrounding C) | ≤8 KB target / 12 KB hard |
| `aplib_p3.bin`          | `-6` | `aplib_depack_mmx.asm` (SSE depacker variant exists in `aplib_depack_sse.asm` but is not wired in; see notes) | ≤10 KB target / 14 KB hard |

On the MMX gate: aPLib match copies use `rep movsb` over ranges where
the source can overlap the destination (`offset < length`, the
canonical compression-of-zeros case). MOVQ doesn't honor that overlap,
so the MMX path only fires when offset >= 8 AND length >= 8. Short
matches stay on the scalar path. EMMS is emitted on exit so any
future x87 user in the stub doesn't see stale MMX tag words.

On SSE for p3: `aplib_depack_sse.asm` ships in the source tree with a
MOVUPS 16-byte block-copy path, but it isn't linked into
`aplib_p3.bin`. Under DOSBox-X 2026.05.02 `cputype=pentium_iii` the
SSE depacker hangs on multi-chunk payloads bigger than the small-
fixture gate. NASM disassembly of the loop encoding looks right, so
the symptom is most likely a DOSBox-X SSE emulation gap rather than a
depacker bug. Verifying on a real Pentium III box or a different
emulator would prove or disprove that. Left for follow-up.

LZMA stub blobs (`lzma_<tier>.bin` for 386 .. p3) are NOT unified
with the aplib blob via runtime dispatch — the LZMA range decoder and
its dictionary buffer don't fit alongside the aplib small-model BSS,
so they get separate per-tier blobs and the host selects the LZMA
blob when `--algo lzma`. Those land later in Phase 5.
