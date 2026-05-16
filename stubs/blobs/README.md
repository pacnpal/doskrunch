Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` and commits the result, so the host
crate stays buildable without the Watcom toolchain installed locally.

Phase 1: `stored_8086.bin` ships first.

Until the Watcom Docker build runs in CI, this directory contains a
syntactically-valid but non-functional placeholder: a minimal MZ header
followed by zero padding. The host `pack` command refuses to write an
SFX from a non-MZ stub, so the placeholder keeps host tests honest
while still failing fast if someone tries to build a real SFX with it.
Replace `stored_8086.bin` with the Watcom-built blob to get a runnable
.EXE.
