//! Embedded prebuilt stub blobs. CI rebuilds these from `stubs/src/` and
//! commits them to `stubs/blobs/`. We `include_bytes!` them so the host
//! binary stays a single static artifact.
//!
//! Phase 1 ships exactly one blob: stored / 8086. Until the Watcom build
//! lands, that file is a placeholder of zeroes; `cargo test` doesn't
//! depend on it being a real executable.

use crate::archive::{Algorithm, TargetTier};

const STORED_8086: &[u8] = include_bytes!("../../stubs/blobs/stored_8086.bin");

/// Returns the prebuilt stub blob for the given (algorithm, tier), or an
/// error if the combination isn't shipped yet.
pub fn stub_for(algo: Algorithm, target: TargetTier) -> Result<&'static [u8], String> {
    match (algo, target) {
        (Algorithm::Stored, TargetTier::I8086) => Ok(STORED_8086),
        (a, t) => Err(format!(
            "no prebuilt stub for ({}, {}); shipped tiers in this phase: (stored, 8086)",
            a.name(),
            t.name()
        )),
    }
}

