//! Compression backends. Phase 2 ships aPLib via the vendored apultra
//! C library. Phase 5 adds LZMA via lzma-rust (host-side encoder) and
//! the vendored xz-embedded C decoder (used both stub-side and host-
//! side via `host/build.rs`). LZSA2 lands in Phase 6.

pub mod aplib;
pub mod lzma;
