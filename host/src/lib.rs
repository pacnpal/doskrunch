//! doskrunch host library API.
//!
//! The CLI binary (`src/main.rs`) builds on top of this. The modules
//! below are re-exported as `pub` so the fuzz harness (and any
//! downstream tooling that wants to read or write doskrunch archives
//! without going through the CLI) can reach the parser at
//! `archive::Archive::read`. Items inside each module remain
//! selectively `pub`; nothing about the surface is stable across
//! phases.

pub mod archive;
pub mod fat_time;
pub mod inspect;
pub mod name83;
pub mod pack;
pub mod stubs;
pub mod unpack;
