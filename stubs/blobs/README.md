Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` (`.github/workflows/build-stubs.yml`)
and fails if the committed copy drifts, so the host crate stays buildable
without the Watcom toolchain installed locally.

Phase 2: `aplib_8086.bin` — Open Watcom v2 + NASM, `-bcl=dos -0 -ms -os`,
linked with `stubs/src/aplib_depack_16.asm`. Dispatches at runtime on the
archive's algorithm byte (0=stored streaming copy, 1=aPLib via the
embedded 145-byte depacker). The host returns the same blob for both
`--algo stored` and `--algo aplib` on `--target 8086`.

The legacy Phase 1 `stored_8086.bin` blob will be removed in the commit
that lands `aplib_8086.bin` — the new blob handles both algorithms.
