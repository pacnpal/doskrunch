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
| `aplib_pentium-mmx.bin` | `-5` | `aplib_depack_p5.asm` (MMX-accelerated depacker copy paths deferred — see notes) | ≤8 KB target / 12 KB hard |
| `aplib_p2.bin`          | `-6` | `aplib_depack_p5.asm` (P6 OoO codegen in C; depacker unchanged) | ≤8 KB target / 12 KB hard |
| `aplib_p3.bin`          | `-6` | `aplib_depack_p5.asm` (SSE-accelerated depacker copy paths deferred) | ≤10 KB target / 14 KB hard |

Why pentium-mmx/p2/p3 ship without vectorized depacker copy paths today:
aPLib literals are emitted one byte at a time (`movsb`) gated on a
bit-decode, and match copies use `rep movsb` over ranges where the
source can overlap the destination (`offset < length`). MOVQ and
MOVAPS don't honor that overlap, so a naive MMX/SSE copy would corrupt
match ranges on the hot path. PLAN.md §10 anticipates a tiered fix
(scalar fallback when `offset < 8` / `offset < 16`, MMX/SSE otherwise);
the PLAN's literal-run case doesn't exist in aPLib's stream the way
it does in LZMA/LZSA, so the practical win is limited to a fraction of
match copies. Karpathy: measure before optimizing; tier blobs ship with
the existing depacker today, and an MMX/SSE depacker variant lands as
a follow-up if benchmarks justify it.

LZMA stub blobs (`lzma_<tier>.bin` for 386 .. p3) are NOT unified
with the aplib blob via runtime dispatch — the LZMA range decoder and
its dictionary buffer don't fit alongside the aplib small-model BSS,
so they get separate per-tier blobs and the host selects the LZMA
blob when `--algo lzma`. Those land later in Phase 5.
