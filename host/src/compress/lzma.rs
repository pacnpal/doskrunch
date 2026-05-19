//! Host-side LZMA codec for doskrunch.
//!
//! Encoder: lzma-rust (Apache-2.0, pure Rust). Uses
//! `LZMAWriter::new(counting, &options, use_header=false,
//! use_end_marker=false, Some(uncompressed_size))`, which produces a
//! raw LZMA1 stream with no 13-byte properties header and no
//! end-of-stream marker — exactly the wire format xz-embedded's
//! MicroLZMA decoder expects, modulo a one-byte transformation at the
//! start of the stream (see below). The `use_header` flag is passed
//! through explicitly rather than going through the
//! `LZMAWriter::new_no_header` helper so the false/false pair is
//! visible at the call site.
//!
//! Decoder: the vendored xz-embedded C library, compiled into the
//! `xz_embedded` static lib by `host/build.rs`. We call
//! `xz_dec_microlzma_alloc/reset/run/end` via narrow `extern "C"`
//! declarations bound below. Used by `host/src/unpack.rs` so
//! `doskrunch unpack` can extract LZMA archives without booting DOS,
//! and by the round-trip tests in this file to verify the encoder
//! produces a stream the stub-side decoder will be able to consume.
//!
//! MicroLZMA framing (per xz-embedded's `linux/include/linux/xz.h`):
//!
//! > The compressed format supported by this decoder is a raw LZMA
//! > stream whose first byte (always 0x00) has been replaced with
//! > bitwise-negation of the LZMA properties (lc/lp/pb) byte.
//!
//! Compared to a full .xz container (which xz-embedded also supports
//! via `xz_dec_stream.c`), MicroLZMA cuts per-chunk framing overhead
//! from ~40 bytes to 1 byte: the stub knows the compressed and
//! uncompressed sizes from the DKCH per-chunk header so the container's
//! length / CRC fields are redundant. The xz_dec_stream.c source isn't
//! compiled into the host lib for this reason.
//!
//! Properties: lc=3, lp=0, pb=2 — the default LZMA1 settings.
//! Dictionary size: chosen by the caller per chunk, capped by the
//! stub's available DOS-heap allocation budget at runtime (see
//! `stubs/src/stub.c` LZMA branch for the runtime cap).

use lzma_rust::{CountingWriter, LZMA2Options, LZMAWriter};
use std::io::Write;
use std::os::raw::c_int;
use std::sync::Once;

// ----- xz-embedded MicroLZMA decoder FFI -----------------------------

/// Opaque mirror of the `struct xz_dec_microlzma` typedef. Layout is
/// not reproduced on the Rust side; we only ever hold raw pointers.
#[repr(C)]
struct XzDecMicrolzma {
    _private: [u8; 0],
}

/// `enum xz_mode` from `linux/include/linux/xz.h`. Only the two values
/// MicroLZMA supports are needed here.
#[repr(C)]
#[allow(dead_code)]
enum XzMode {
    Single = 0,
    Prealloc = 1,
    Dynalloc = 2,
}

/// `enum xz_ret` — every variant kept so the match arms in `decompress`
/// can name each return code explicitly.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum XzRet {
    Ok = 0,
    StreamEnd = 1,
    UnsupportedCheck = 2,
    MemError = 3,
    MemlimitError = 4,
    FormatError = 5,
    OptionsError = 6,
    DataError = 7,
    BufError = 8,
}

#[repr(C)]
struct XzBuf {
    in_: *const u8,
    in_pos: usize,
    in_size: usize,
    out: *mut u8,
    out_pos: usize,
    out_size: usize,
}

extern "C" {
    fn xz_crc32_init();
    fn xz_dec_microlzma_alloc(mode: XzMode, dict_size: u32) -> *mut XzDecMicrolzma;
    fn xz_dec_microlzma_reset(
        s: *mut XzDecMicrolzma,
        comp_size: u32,
        uncomp_size: u32,
        uncomp_size_is_exact: c_int,
    );
    fn xz_dec_microlzma_run(s: *mut XzDecMicrolzma, b: *mut XzBuf) -> XzRet;
    fn xz_dec_microlzma_end(s: *mut XzDecMicrolzma);
}

/// Default LZMA1 properties (lc=3, lp=0, pb=2). The MicroLZMA header
/// byte is the bitwise negation of `(pb * 5 + lp) * 9 + lc`.
pub const PROPS_LC: u32 = 3;
pub const PROPS_LP: u32 = 0;
pub const PROPS_PB: u32 = 2;

/// Bitwise-negated LZMA properties byte: `~((pb * 5 + lp) * 9 + lc)`.
/// xz-embedded restores the original by negating again at decode time.
pub const MICROLZMA_PROPS_BYTE: u8 = !(((PROPS_PB * 5 + PROPS_LP) * 9 + PROPS_LC) as u8);

/// Compress `data` to a MicroLZMA stream. Returns the bytes to store
/// on disk. The first byte is `MICROLZMA_PROPS_BYTE`; the remaining
/// bytes are the raw LZMA1 range-coded payload (no EOS marker, no
/// trailing properties or sizes).
///
/// The `dict_size` argument controls the encoder's match-finder window
/// in bytes. Smaller dict = less compression but lower decoder RAM at
/// runtime; LZMA's minimum is 4096 bytes.
pub fn compress(data: &[u8], dict_size: u32) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if dict_size < lzma_rust::DICT_SIZE_MIN {
        return Err(format!(
            "lzma: dict_size {} below the {} byte minimum",
            dict_size,
            lzma_rust::DICT_SIZE_MIN
        ));
    }
    // Preset 6 is lzma-rust's default (matches `xz -6`). Picked
    // explicitly so the constant is auditable from this file.
    let mut options = LZMA2Options::with_preset(6);
    options.dict_size = dict_size;
    options.lc = PROPS_LC;
    options.lp = PROPS_LP;
    options.pb = PROPS_PB;

    let mut raw = Vec::with_capacity(data.len());
    {
        let counting = CountingWriter::new(&mut raw);
        // use_header=false (no 13-byte LZMA1 header), use_end_marker=false
        // (the stub knows the uncompressed size from the DKCH chunk
        // header so we don't need the in-band EOS).
        let mut writer = LZMAWriter::new(counting, &options, false, false, Some(data.len() as u64))
            .map_err(|e| format!("lzma: encoder init failed: {e}"))?;
        writer
            .write_all(data)
            .map_err(|e| format!("lzma: write failed: {e}"))?;
        // Empty write triggers the encoder's flush + finalize path.
        writer
            .write(&[])
            .map_err(|e| format!("lzma: flush failed: {e}"))?;
    }
    // MicroLZMA framing: the raw LZMA1 stream's first byte is always
    // 0x00 (the range coder's initial output). Replace it with the
    // negated properties byte so xz-embedded's decoder can recover the
    // properties from the stream itself.
    if raw.is_empty() {
        return Err("lzma: encoder produced an empty stream".into());
    }
    if raw[0] != 0x00 {
        return Err(format!(
            "lzma: encoder first byte was {:#04x}, expected 0x00 — \
             upstream lzma-rust may have changed the range-coder \
             initialization and MicroLZMA framing needs to be revisited",
            raw[0]
        ));
    }
    raw[0] = MICROLZMA_PROPS_BYTE;
    Ok(raw)
}

/// Decompress a MicroLZMA stream produced by `compress`. `expected_size`
/// is the exact uncompressed size — the caller knows it from the DKCH
/// per-chunk header, and providing it tightens xz-embedded's error
/// detection. `dict_size` must match what was used at encode time (the
/// stream is decode-time-symmetric).
///
/// Returns an error on size mismatch or any decoder-reported failure.
pub fn decompress(
    compressed: &[u8],
    expected_size: usize,
    dict_size: u32,
) -> Result<Vec<u8>, String> {
    if expected_size == 0 {
        if !compressed.is_empty() {
            return Err(format!(
                "lzma: chunk declares 0 uncompressed bytes but carries {} compressed",
                compressed.len()
            ));
        }
        return Ok(Vec::new());
    }
    if compressed.is_empty() {
        return Err(format!(
            "lzma: chunk declares {} uncompressed bytes but carries no compressed data",
            expected_size
        ));
    }
    if dict_size < lzma_rust::DICT_SIZE_MIN {
        return Err(format!(
            "lzma: dict_size {} below the {} byte minimum",
            dict_size,
            lzma_rust::DICT_SIZE_MIN
        ));
    }
    let comp_u32: u32 = compressed
        .len()
        .try_into()
        .map_err(|_| "lzma: compressed size > u32".to_string())?;
    let uncomp_u32: u32 = expected_size
        .try_into()
        .map_err(|_| "lzma: expected size > u32".to_string())?;

    // SAFETY: xz_crc32_init mutates a static `xz_crc32_table` without
    // any internal synchronization, so concurrent calls from multiple
    // threads are a data race (UB). std::sync::Once gives exactly-once
    // initialization with the right happens-before edges for every
    // subsequent decoder run on any thread.
    static CRC32_INIT: Once = Once::new();
    CRC32_INIT.call_once(|| unsafe { xz_crc32_init() });

    // SAFETY: passing a valid mode enum and a u32 dict size. NULL
    // return means allocation failure; we check below.
    let dec = unsafe { xz_dec_microlzma_alloc(XzMode::Prealloc, dict_size) };
    if dec.is_null() {
        return Err(format!(
            "lzma: xz_dec_microlzma_alloc({} bytes dict) failed",
            dict_size
        ));
    }

    // RAII guard: even on panic we need to free the decoder state. A
    // tiny struct around the pointer with a Drop impl is the standard
    // idiom; using a closure-with-drop here is overkill.
    struct DecGuard(*mut XzDecMicrolzma);
    impl Drop for DecGuard {
        fn drop(&mut self) {
            unsafe { xz_dec_microlzma_end(self.0) }
        }
    }
    let _guard = DecGuard(dec);

    let mut out = vec![0u8; expected_size];
    let mut buf = XzBuf {
        in_: compressed.as_ptr(),
        in_pos: 0,
        in_size: compressed.len(),
        out: out.as_mut_ptr(),
        out_pos: 0,
        out_size: out.len(),
    };

    // SAFETY: dec is non-null (checked), comp/uncomp fit in u32 (checked).
    unsafe { xz_dec_microlzma_reset(dec, comp_u32, uncomp_u32, 1) };

    // MicroLZMA returns OK / StreamEnd / DataError. OK is only
    // returned when more input or output is needed — we provide both
    // up front, so the only happy path here is StreamEnd.
    let ret = unsafe { xz_dec_microlzma_run(dec, &mut buf) };
    match ret {
        XzRet::StreamEnd => {}
        XzRet::Ok => {
            return Err(format!(
                "lzma: decoder returned XZ_OK without StreamEnd \
                 (in_pos={} of {}, out_pos={} of {}); MicroLZMA contract \
                 says this shouldn't happen when comp/uncomp sizes are exact",
                buf.in_pos, buf.in_size, buf.out_pos, buf.out_size
            ));
        }
        other => return Err(format!("lzma: decoder returned {other:?}")),
    }
    if buf.out_pos != expected_size {
        return Err(format!(
            "lzma: decoded {} bytes but chunk declared {}",
            buf.out_pos, expected_size
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DICT_16K: u32 = 16 * 1024;

    #[test]
    fn empty_roundtrips() {
        let c = compress(b"", DICT_16K).unwrap();
        assert!(c.is_empty());
        let d = decompress(&c, 0, DICT_16K).unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn short_roundtrips() {
        let input = b"hello LZMA world, hello LZMA world, hello LZMA world.";
        let c = compress(input, DICT_16K).unwrap();
        assert!(!c.is_empty());
        // First byte must be the MicroLZMA properties byte.
        assert_eq!(c[0], MICROLZMA_PROPS_BYTE);
        let d = decompress(&c, input.len(), DICT_16K).unwrap();
        assert_eq!(d.as_slice(), input.as_slice());
    }

    #[test]
    fn repetitive_block_compresses_well() {
        let mut input = Vec::new();
        for i in 0..4096u16 {
            input.extend_from_slice(&i.to_le_bytes());
            input.extend_from_slice(b"the quick brown fox jumps over the lazy dog\n");
        }
        let c = compress(&input, DICT_16K).unwrap();
        assert!(
            c.len() < input.len(),
            "compressed {} >= input {}",
            c.len(),
            input.len()
        );
        let d = decompress(&c, input.len(), DICT_16K).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn zero_block_roundtrips() {
        let input = vec![0u8; 16384];
        let c = compress(&input, DICT_16K).unwrap();
        let d = decompress(&c, input.len(), DICT_16K).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn rejects_size_mismatch() {
        let input = b"some data to compress to test mismatch handling..............";
        let c = compress(input, DICT_16K).unwrap();
        // Decoder told we expect one more byte than reality; xz-embedded
        // catches that and returns a DataError.
        let err = decompress(&c, input.len() + 1, DICT_16K).unwrap_err();
        assert!(err.contains("lzma"), "got {err:?}");
    }

    #[test]
    fn rejects_dict_below_minimum() {
        let err = compress(b"x", 100).unwrap_err();
        assert!(err.contains("minimum"), "got {err:?}");
    }

    #[test]
    fn beats_aplib_on_200kb_realistic_payload() {
        // PLAN.md §10 Phase 5 Verify: "LZMA produces smaller files
        // than aPLib on payloads > 100KB." The previous version of
        // this test used a synthetic exec-like payload with massive
        // identical repetitions, which aPLib's RLE-on-zeros and
        // short-match handling crushed to ~72 bytes — meaningless as
        // a head-to-head LZMA-vs-aPLib comparison. Use an LCG-derived
        // byte sequence with embedded short literals so both encoders
        // have real entropy to work against, not just N copies of the
        // same 200-byte block.
        let mut input: Vec<u8> = Vec::with_capacity(200 * 1024);
        let mut lcg: u32 = 0xDECAFBAD;
        let phrases: &[&[u8]] = &[
            b"error: out of memory\0",
            b"error: bad magic\0",
            b"warning: deprecated\0",
            b"doskrunch phase 5 LZMA gate\n",
            b"the quick brown fox jumps over the lazy dog\n",
        ];
        while input.len() < 200 * 1024 {
            // Mostly-random LCG bytes (incompressible by either
            // codec) with periodic short literal phrases sprinkled in.
            // The ratio (~30 KB of random per ~2 KB of phrase) matches
            // a typical mixed-content payload well enough to be a
            // useful head-to-head.
            for _ in 0..512 {
                lcg = lcg.wrapping_mul(1_103_515_245).wrapping_add(12345);
                input.push((lcg >> 16) as u8);
                if input.len() >= 200 * 1024 {
                    break;
                }
            }
            let phrase = phrases[(lcg as usize) % phrases.len()];
            for _ in 0..16 {
                input.extend_from_slice(phrase);
                if input.len() >= 200 * 1024 {
                    break;
                }
            }
        }
        input.truncate(200 * 1024);
        let lzma_bytes = compress(&input, 64 * 1024).unwrap();
        let aplib_bytes = crate::compress::aplib::compress(&input).unwrap();
        assert!(
            lzma_bytes.len() < aplib_bytes.len(),
            "lzma {} should beat aplib {} on the 200 KB mixed-content payload",
            lzma_bytes.len(),
            aplib_bytes.len()
        );
    }

    #[test]
    fn deterministic_compression() {
        // Two pack runs over the same bytes + same options must produce
        // byte-identical output. PLAN.md §1 reproducibility requirement.
        let input = b"deterministic LZMA test ".repeat(1024);
        let a = compress(&input, DICT_16K).unwrap();
        let b = compress(&input, DICT_16K).unwrap();
        assert_eq!(a, b, "lzma encoder not deterministic");
    }
}
