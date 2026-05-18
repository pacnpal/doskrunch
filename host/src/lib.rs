//! doskrunch host library API.
//!
//! The CLI binary (`src/main.rs`) builds on top of this. The library is
//! also what fuzz targets and downstream tools link against — the public
//! surface area is currently the archive container parser/encoder.
//!
//! Everything is `pub` so the fuzz harness can call `Archive::read`
//! without going through the CLI. The shape of the API is unstable
//! across phases.

pub mod archive;
pub mod fat_time;
pub mod inspect;
pub mod name83;
pub mod pack;
pub mod stubs;
pub mod unpack;
