//! Embedded prebuilt stub blobs. CI rebuilds these from `stubs/src/` and
//! commits them to `stubs/blobs/`. We `include_bytes!` them so the host
//! binary stays a single static artifact.
//!
//! Phase 5 ships eight aplib blobs (Phase 3 shipped three):
//!
//!   * `aplib_8086.bin`        — wcc `-0` + `aplib_depack_16.asm`.
//!   * `aplib_286.bin`         — wcc `-2` + `aplib_depack_16.asm` (the
//!     16-bit depacker is `cpu 8086`, a strict subset of 286).
//!   * `aplib_386.bin`         — wcc `-3` + `aplib_depack_32.asm`.
//!   * `aplib_486.bin`         — wcc `-4` + `aplib_depack_32.asm` (the
//!     32-bit depacker is `cpu 386`, a strict subset of 486).
//!   * `aplib_pentium.bin`     — wcc `-5` + `aplib_depack_p5.asm`.
//!   * `aplib_pentium-mmx.bin` — wcc `-5` + `aplib_depack_mmx.asm` (MMX
//!     8-byte block copy when match offset and length are both >= 8;
//!     scalar `rep movsb` for shorter or overlapping matches; EMMS on
//!     exit so a future x87 user can't see stale MMX tag words).
//!   * `aplib_p2.bin`          — wcc `-6` + `aplib_depack_mmx.asm`.
//!   * `aplib_p3.bin`          — wcc `-6` + `aplib_depack_mmx.asm`. An
//!     SSE-accelerated depacker variant lives in
//!     `stubs/src/aplib_depack_sse.asm` but is NOT linked in: under
//!     DOSBox-X 2026.05.02 `cputype=pentium_iii` the MOVUPS-based block
//!     copy hangs on multi-chunk payloads; left for follow-up. See
//!     `stubs/blobs/README.md` for the full deferral note.
//!
//! Each `aplib_<tier>.bin` is a complete Watcom-built DOS .EXE that
//! dispatches at runtime on the archive's algorithm byte and handles
//! both `stored` (0) and `aplib` (1) chunks. The host returns the
//! matching aplib blob for both `Algorithm::Stored` and
//! `Algorithm::Aplib` on the requested `--target` tier.
//!
//! Phase 5 also ships six LZMA blobs (`lzma_<tier>.bin` for 386..p3).
//! These are LZMA-only — they require the archive's algorithm byte
//! to be `lzma` (3) and die loudly on anything else. The host's
//! `stub_for()` routes `Algorithm::Lzma` at 386+ to the matching
//! `lzma_<tier>.bin` and never to an aplib blob. The LZMA blobs are
//! NOT unified with the aplib blob via runtime dispatch because their
//! working-set footprint (LZMA range decoder + dict buffer) blows
//! past the aplib stub's small-model BSS budget; the LZMA stub is
//! built compact-model (`-mc`) for the same reason. See
//! `stubs/blobs/README.md` for the per-tier matrix and size budgets.

use crate::archive::{Algorithm, TargetTier};

const APLIB_8086: &[u8] = include_bytes!("../../stubs/blobs/aplib_8086.bin");
const APLIB_286: &[u8] = include_bytes!("../../stubs/blobs/aplib_286.bin");
const APLIB_386: &[u8] = include_bytes!("../../stubs/blobs/aplib_386.bin");
const APLIB_486: &[u8] = include_bytes!("../../stubs/blobs/aplib_486.bin");
const APLIB_PENTIUM: &[u8] = include_bytes!("../../stubs/blobs/aplib_pentium.bin");
const APLIB_PENTIUM_MMX: &[u8] = include_bytes!("../../stubs/blobs/aplib_pentium-mmx.bin");
const APLIB_P2: &[u8] = include_bytes!("../../stubs/blobs/aplib_p2.bin");
const APLIB_P3: &[u8] = include_bytes!("../../stubs/blobs/aplib_p3.bin");

const LZMA_386: &[u8] = include_bytes!("../../stubs/blobs/lzma_386.bin");
const LZMA_486: &[u8] = include_bytes!("../../stubs/blobs/lzma_486.bin");
const LZMA_PENTIUM: &[u8] = include_bytes!("../../stubs/blobs/lzma_pentium.bin");
const LZMA_PENTIUM_MMX: &[u8] = include_bytes!("../../stubs/blobs/lzma_pentium-mmx.bin");
const LZMA_P2: &[u8] = include_bytes!("../../stubs/blobs/lzma_p2.bin");
const LZMA_P3: &[u8] = include_bytes!("../../stubs/blobs/lzma_p3.bin");

/// Returns the prebuilt stub blob for the given (algorithm, tier), or an
/// error if the combination isn't shipped yet.
///
/// Stored / aplib / lzsa2 all share the per-tier aplib blob — the
/// stub dispatches at runtime on the archive's algo byte (Phase 6
/// linked `lzsa2_depack` alongside `aplib_depack` in each of the
/// eight tier blobs). LZMA stays on its own per-tier blob because
/// its decoder state + dict don't fit in small-model DS.
pub fn stub_for(algo: Algorithm, target: TargetTier) -> Result<&'static [u8], String> {
    match (algo, target) {
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::I8086) => {
            Ok(APLIB_8086)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::I286) => {
            Ok(APLIB_286)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::I386) => {
            Ok(APLIB_386)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::I486) => {
            Ok(APLIB_486)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::Pentium) => {
            Ok(APLIB_PENTIUM)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::PentiumMmx) => {
            Ok(APLIB_PENTIUM_MMX)
        }
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::P2) => Ok(APLIB_P2),
        (Algorithm::Stored | Algorithm::Aplib | Algorithm::Lzsa2, TargetTier::P3) => Ok(APLIB_P3),
        (Algorithm::Lzma, TargetTier::I386) => Ok(LZMA_386),
        (Algorithm::Lzma, TargetTier::I486) => Ok(LZMA_486),
        (Algorithm::Lzma, TargetTier::Pentium) => Ok(LZMA_PENTIUM),
        (Algorithm::Lzma, TargetTier::PentiumMmx) => Ok(LZMA_PENTIUM_MMX),
        (Algorithm::Lzma, TargetTier::P2) => Ok(LZMA_P2),
        (Algorithm::Lzma, TargetTier::P3) => Ok(LZMA_P3),
        (a, t) => Err(format!(
            "no prebuilt stub for ({}, {}); shipped in this build: \
             (stored|aplib|lzsa2, 8086|286|386|486|pentium|pentium-mmx|p2|p3) \
             + (lzma, 386|486|pentium|pentium-mmx|p2|p3)",
            a.name(),
            t.name()
        )),
    }
}
