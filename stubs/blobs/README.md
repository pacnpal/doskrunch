Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` (`.github/workflows/build-stubs.yml`)
and fails if the committed copy drifts, so the host crate stays buildable
without the Watcom toolchain installed locally.

Phase 3 ships three blobs — one per CPU target tier. All use Open
Watcom v2 (`wcc -bt=dos -ms -os` + `wlink system dos`) and NASM 2.x
(`-f obj`). Each dispatches at runtime on the archive's algorithm byte
(0=stored streaming copy, 1=aPLib via the linked depacker), so the
host returns the same blob for both `--algo stored` and `--algo aplib`
on the requested `--target`.

| Blob | wcc flag | aPLib depacker | Size budget |
|------|----------|----------------|-------------|
| `aplib_8086.bin` | `-0` | `aplib_depack_16.asm` (145 B 8088 port) | ≤4 KB target / 8 KB hard |
| `aplib_386.bin`  | `-3` | `aplib_depack_32.asm` (203 B 32-bit size-opt port) | ≤6 KB target / 10 KB hard |
| `aplib_pentium.bin` | `-5` | `aplib_depack_p5.asm` (252 B 32-bit speed-opt port, no manual U/V scheduling in this revision) | ≤8 KB target / 12 KB hard |
