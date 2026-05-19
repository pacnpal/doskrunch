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
| `aplib_p3.bin`          | `-6` | `aplib_depack_sse.asm` (SSE MOVUPS 16-byte block copy when offset >= 16 and length >= 16; scalar `rep movsb` otherwise) | ≤10 KB target / 14 KB hard |

On the MMX gate: aPLib match copies use `rep movsb` over ranges where
the source can overlap the destination (`offset < length`, the
canonical compression-of-zeros case). MOVQ doesn't honor that overlap,
so the MMX path only fires when offset >= 8 AND length >= 8. Short
matches stay on the scalar path. EMMS is emitted on exit so any
future x87 user in the stub doesn't see stale MMX tag words.

On SSE for p3: real-mode XMM use requires CR4.OSFXSR=1 and CR0.EM/TS
cleared before any MOVUPS executes. `aplib_depack_sse.asm` enables
those bits in its prologue; without that, strict emulators/real CPUs
fault with #UD on first SSE instruction. This path is validated on
QEMU `-cpu pentium3` with a 500 KiB multi-chunk payload at chunk sizes
8, 64, 4096, and 16384 (byte-identical extraction).

LZMA stub blobs ship now for 386..p3. They are NOT unified with the
aplib blob via runtime dispatch — the LZMA range decoder state, the
LZMA dictionary buffer, and the per-chunk scratch buffers don't fit
alongside the aplib stub's small-model BSS, so the LZMA stub is its
own program. The host's `stub_for` selects `lzma_<tier>.bin` when
`--algo lzma --target <tier>` is requested.

LZMA stubs use Open Watcom's compact memory model (`-mc`, near code,
far data) — `struct xz_dec_microlzma` alone exceeds small-model
malloc's 32 KB per-allocation cap, so the decoder state lives in a
data segment distinct from the stub's BSS. The aplib stubs stay on
small (`-ms`).

| Blob | wcc flag | Memory model | Linked objects | Size budget |
|------|----------|--------------|----------------|-------------|
| `lzma_386.bin`          | `-3` | compact (`-mc`) | `stub_lzma.obj` + `xz_crc32.obj` + `xz_dec_lzma2.obj` | ≤18 KB target / 24 KB hard |
| `lzma_486.bin`          | `-4` | compact (`-mc`) | same | ≤18 KB target / 24 KB hard |
| `lzma_pentium.bin`      | `-5` | compact (`-mc`) | same | ≤18 KB target / 24 KB hard |
| `lzma_pentium-mmx.bin`  | `-5` | compact (`-mc`) | same | ≤18 KB target / 24 KB hard |
| `lzma_p2.bin`           | `-6` | compact (`-mc`) | same | ≤18 KB target / 24 KB hard |
| `lzma_p3.bin`           | `-6` | compact (`-mc`) | same | ≤20 KB target / 28 KB hard |

The LZMA blob hard ceilings (24/28 KB) intentionally exceed the
per-tier aplib ceilings (10/12/14 KB). The LZMA decoder + dictionary
+ scratch buffers are inherently larger than the aPLib decoder, and
treating the LZMA stub as its own size class is the explicit Phase 5
contract — opting into `--algo lzma` is opting into a bigger stub.
The aplib stubs continue to enforce their original per-tier ceilings
in `stubs/Makefile`.

A 16-bit C portability note: the vendored xz-embedded source has two
`/* doskrunch patch: */`-marked changes in `xz_dec_lzma2.c` for `1 <<
24` and `3U << 30`, both of which are undefined when `int` is 16 bits
(Open Watcom's default in real mode). The patches are local; the rest
of xz-embedded is unchanged from upstream.
