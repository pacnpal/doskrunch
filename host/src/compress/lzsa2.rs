//! Host-side LZSA2 codec for doskrunch.
//!
//! Encoder and decoder both come from the vendored lzsa library
//! (`vendor/lzsa/`, zlib license). lzsa exposes `lzsa_compress_inmem` /
//! `lzsa_decompress_inmem`; we pass `LZSA_FLAG_RAW_BLOCK` so they emit
//! and consume raw LZSA2 blocks with no enclosing frame, matching what
//! the stub-side ASM decoders (`stubs/src/lzsa2_depack_{16,32}.asm`)
//! expect on input. The doskrunch archive carries the per-chunk
//! uncompressed/compressed sizes in the DKCH framing already, so the
//! lzsa frame header would be redundant.
//!
//! Settings: format version 2 (LZSA2), minimum match size 2 (the
//! LZSA2 default per `vendor/lzsa/src/format.h::MIN_MATCH_SIZE_V2`),
//! favor-ratio flag set so the encoder picks the slower-but-tighter
//! search path. The fast (favor-speed) variant matters more for
//! pipelined CLI tools than for doskrunch's "pack once, ship the
//! SFX" workflow.

use std::os::raw::{c_int, c_uchar, c_uint};

const LZSA_FLAG_FAVOR_RATIO: c_uint = 1 << 0;
const LZSA_FLAG_RAW_BLOCK: c_uint = 1 << 1;
const LZSA_MIN_MATCH_SIZE_V2: c_int = 2;
const LZSA_FORMAT_VERSION_V2: c_int = 2;
/// lzsa's "compressor returned an error" sentinel: `(size_t)-1`.
const LZSA_ERROR: usize = usize::MAX;

extern "C" {
    fn lzsa_get_max_compressed_size_inmem(input_size: usize) -> usize;

    fn lzsa_compress_inmem(
        input: *mut c_uchar,
        output: *mut c_uchar,
        input_size: usize,
        max_output_size: usize,
        flags: c_uint,
        min_match_size: c_int,
        format_version: c_int,
    ) -> usize;

    fn lzsa_decompress_inmem(
        file_data: *mut c_uchar,
        output: *mut c_uchar,
        file_size: usize,
        max_output_size: usize,
        flags: c_uint,
        format_version: *mut c_int,
    ) -> usize;
}

/// Compress `data` to a raw LZSA2 block. Returns the bytes to store on
/// disk; the stub's `lzsa2_depack` consumes them verbatim.
///
/// Empty input returns `Ok(Vec::new())` — lzsa doesn't define behavior
/// for 0-byte input and the only in-tree caller (`build_lzsa2_entry`)
/// already special-cases empty files. New callers MUST treat the empty
/// `Vec` as "nothing to encode"; passing it to `decompress(&[], n > 0)`
/// will fail.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let max_out = unsafe { lzsa_get_max_compressed_size_inmem(data.len()) };
    let mut buf = vec![0u8; max_out];
    // lzsa_compress_inmem takes the input as a mut pointer (it doesn't
    // write to it, but the API isn't const-correct). We feed it a copy
    // via to_vec so the caller's slice stays untouched even if lzsa
    // ever starts writing to it.
    let mut input_copy = data.to_vec();
    let written = unsafe {
        lzsa_compress_inmem(
            input_copy.as_mut_ptr(),
            buf.as_mut_ptr(),
            input_copy.len(),
            buf.len(),
            LZSA_FLAG_RAW_BLOCK | LZSA_FLAG_FAVOR_RATIO,
            LZSA_MIN_MATCH_SIZE_V2,
            LZSA_FORMAT_VERSION_V2,
        )
    };
    if written == LZSA_ERROR {
        return Err(format!(
            "lzsa2: lzsa_compress_inmem failed (input {} bytes, output buffer {} bytes)",
            data.len(),
            buf.len()
        ));
    }
    buf.truncate(written);
    Ok(buf)
}

/// Decompress a raw LZSA2 block produced by `compress` into a buffer
/// of the exact expected size. Returns an error on size mismatch or
/// any lzsa-reported failure.
pub fn decompress(compressed: &[u8], expected_size: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; expected_size];
    if expected_size == 0 {
        if !compressed.is_empty() {
            return Err(format!(
                "lzsa2: chunk declares 0 uncompressed bytes but carries {} compressed",
                compressed.len()
            ));
        }
        return Ok(buf);
    }
    if compressed.is_empty() {
        return Err(format!(
            "lzsa2: chunk declares {} uncompressed bytes but carries no compressed data",
            expected_size
        ));
    }
    let mut input_copy = compressed.to_vec();
    let mut fmt_version: c_int = LZSA_FORMAT_VERSION_V2;
    let produced = unsafe {
        lzsa_decompress_inmem(
            input_copy.as_mut_ptr(),
            buf.as_mut_ptr(),
            input_copy.len(),
            buf.len(),
            LZSA_FLAG_RAW_BLOCK,
            &mut fmt_version,
        )
    };
    if produced == LZSA_ERROR {
        return Err(format!(
            "lzsa2: lzsa_decompress_inmem failed (input {} bytes, expected {} bytes out)",
            compressed.len(),
            expected_size
        ));
    }
    if produced != expected_size {
        return Err(format!(
            "lzsa2: decompressed {} bytes but chunk declared {}",
            produced, expected_size
        ));
    }
    if fmt_version != LZSA_FORMAT_VERSION_V2 {
        return Err(format!(
            "lzsa2: decoder reported format version {} (expected {})",
            fmt_version, LZSA_FORMAT_VERSION_V2
        ));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_roundtrips() {
        let c = compress(b"").unwrap();
        assert!(c.is_empty());
        let d = decompress(&c, 0).unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn short_roundtrips() {
        let input = b"hello LZSA2 world, hello LZSA2 world, hello LZSA2 world.";
        let c = compress(input).unwrap();
        assert!(!c.is_empty());
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d.as_slice(), input.as_slice());
    }

    #[test]
    fn repetitive_block_compresses_well() {
        // LZSA2 raw-block mode encodes one block per call (BLOCK_SIZE ≈
        // 64 KiB upstream). Our archive carves payloads into 16 KiB
        // chunks before feeding the encoder, so the test stays under
        // the per-block limit too — anything larger would surface as
        // "lzsa_compress_inmem failed" because raw-block mode doesn't
        // emit multi-block framing.
        let mut input = Vec::new();
        for i in 0..256u16 {
            input.extend_from_slice(&i.to_le_bytes());
            input.extend_from_slice(b"the quick brown fox jumps over the lazy dog\n");
        }
        assert!(
            input.len() < 16 * 1024,
            "test input ({}) must stay under LZSA2_CHUNK_INPUT",
            input.len()
        );
        let c = compress(&input).unwrap();
        assert!(
            c.len() < input.len(),
            "compressed {} >= input {}",
            c.len(),
            input.len()
        );
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn zero_block_roundtrips() {
        let input = vec![0u8; 16384];
        let c = compress(&input).unwrap();
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn rejects_size_mismatch() {
        let input = b"some data to compress to test mismatch handling..............";
        let c = compress(input).unwrap();
        let err = decompress(&c, input.len() + 1).unwrap_err();
        assert!(err.contains("lzsa2"));
    }

    #[test]
    fn deterministic_compression() {
        // Two encode runs over the same bytes must produce byte-identical
        // output. PLAN.md §1 reproducibility requirement.
        let input = b"deterministic LZSA2 test ".repeat(1024);
        let a = compress(&input).unwrap();
        let b = compress(&input).unwrap();
        assert_eq!(a, b, "lzsa2 encoder not deterministic");
    }
}
