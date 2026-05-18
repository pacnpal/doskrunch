//! Embedded prebuilt stub blobs. CI rebuilds these from `stubs/src/` and
//! commits them to `stubs/blobs/`. We `include_bytes!` them so the host
//! binary stays a single static artifact.
//!
//! Phase 3 ships three blobs, one per CPU target tier:
//!
//!   * `aplib_8086.bin`   — wcc `-0` + `aplib_depack_16.asm` (8088-safe).
//!   * `aplib_386.bin`    — wcc `-3` + `aplib_depack_32.asm` (32-bit
//!     register depacker under bits-16 real mode).
//!   * `aplib_pentium.bin` — wcc `-5` + `aplib_depack_p5.asm` (speed-
//!     optimized fast-variant port, no manual U/V scheduling in this
//!     revision).
//!
//! Each blob is a complete Watcom-built DOS .EXE that dispatches at
//! runtime on the archive's algorithm byte and handles both `stored`
//! (0) and `aplib` (1) chunks. The host returns the matching blob for
//! both `Algorithm::Stored` and `Algorithm::Aplib` on the requested
//! `--target` tier.
//!
//! Tiers `286`, `486`, `pentium-mmx`, `p2`, `p3` and the `lzsa2` /
//! `lzma` algorithms remain Phase 5/6 work; `stub_for` returns the
//! same "not shipped" error for them as in Phase 2.

use crate::archive::{Algorithm, TargetTier};

const APLIB_8086: &[u8] = include_bytes!("../../stubs/blobs/aplib_8086.bin");
const APLIB_386: &[u8] = include_bytes!("../../stubs/blobs/aplib_386.bin");
const APLIB_PENTIUM: &[u8] = include_bytes!("../../stubs/blobs/aplib_pentium.bin");

/// Returns the prebuilt stub blob for the given (algorithm, tier), or an
/// error if the combination isn't shipped yet.
pub fn stub_for(algo: Algorithm, target: TargetTier) -> Result<&'static [u8], String> {
    match (algo, target) {
        (Algorithm::Stored, TargetTier::I8086) | (Algorithm::Aplib, TargetTier::I8086) => {
            Ok(APLIB_8086)
        }
        (Algorithm::Stored, TargetTier::I386) | (Algorithm::Aplib, TargetTier::I386) => {
            Ok(APLIB_386)
        }
        (Algorithm::Stored, TargetTier::Pentium) | (Algorithm::Aplib, TargetTier::Pentium) => {
            Ok(APLIB_PENTIUM)
        }
        (a, t) => Err(format!(
            "no prebuilt stub for ({}, {}); shipped tiers in this phase: (stored|aplib, 8086|386|pentium)",
            a.name(),
            t.name()
        )),
    }
}
