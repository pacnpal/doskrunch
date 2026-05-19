//! Compression backends. Phase 2 ships aPLib via the vendored apultra
//! C library. Phase 5 adds LZMA via lzma-rust (host-side encoder) and
//! the vendored xz-embedded C decoder (used both stub-side and host-
//! side via `host/build.rs`). Phase 6 adds LZSA2 via the vendored lzsa
//! C library (encoder + decoder, raw-block mode).

pub mod aplib;
pub mod lzma;
pub mod lzsa2;
