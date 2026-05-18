Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` (`.github/workflows/build-stubs.yml`)
and fails if the committed copy drifts, so the host crate stays buildable
without the Watcom toolchain installed locally.

Phase 1: `stored_8086.bin` — Open Watcom v2, `-bcl=dos -0 -ms -os`.
