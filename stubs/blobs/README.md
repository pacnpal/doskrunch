Prebuilt stub binaries.

Each `<algo>_<tier>.bin` is a complete DOS .EXE (MZ + load image) that
locates its trailing doskrunch archive at runtime via the DKTR trailer.
The Rust host appends the archive + trailer to one of these blobs.

CI rebuilds these from `stubs/src/` and commits the result, so the host
crate stays buildable without the Watcom toolchain installed locally.

Phase 1: `stored_8086.bin` ships first.

Until the Watcom Docker build runs in CI, this directory contains a
non-functional placeholder: a minimal MZ header followed by zero
padding. The host `pack` checks for the MZ magic, so the placeholder
is accepted and host-side roundtrip tests work, **but the resulting
.EXE is not runnable on DOS** — there is no decoder code behind the
MZ header. Replace `stored_8086.bin` with the Watcom-built blob to
get a runnable SFX.
