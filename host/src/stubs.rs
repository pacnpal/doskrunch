//! Embedded prebuilt stub blobs. CI rebuilds these from `stubs/src/` and
//! commits them to `stubs/blobs/`. We `include_bytes!` them so the host
//! binary stays a single static artifact.
//!
//! Phase 2 ships one blob: `aplib_8086.bin`, a Watcom-built DOS .EXE that
//! decompresses both `stored` and `aplib` chunks at runtime (dispatched on
//! the archive's algorithm byte). The host returns the same blob for both
//! `Algorithm::Stored` and `Algorithm::Aplib` on the 8086 target.

use crate::archive::{Algorithm, TargetTier};

const APLIB_8086: &[u8] = include_bytes!("../../stubs/blobs/aplib_8086.bin");

/// Returns the prebuilt stub blob for the given (algorithm, tier), or an
/// error if the combination isn't shipped yet.
pub fn stub_for(algo: Algorithm, target: TargetTier) -> Result<&'static [u8], String> {
    match (algo, target) {
        (Algorithm::Stored, TargetTier::I8086) | (Algorithm::Aplib, TargetTier::I8086) => {
            Ok(APLIB_8086)
        }
        (a, t) => Err(format!(
            "no prebuilt stub for ({}, {}); shipped tiers in this phase: (stored|aplib, 8086)",
            a.name(),
            t.name()
        )),
    }
}

