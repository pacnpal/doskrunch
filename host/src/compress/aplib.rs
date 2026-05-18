//! Rust binding over the vendored apultra aPLib codec.
//!
//! Two narrow `extern "C"` calls — `apultra_compress` and
//! `apultra_decompress` — bridged through small safe wrappers that
//! own their output buffers. apultra's output stream is fully
//! compatible with the original aPLib format by Jørgen Ibsen, which
//! is what the 8086 stub's depacker (`stubs/src/aplib_depack_16.asm`,
//! ported from `vendor/apultra/asm/8088/aplib_8088_small.S`) decodes.

use std::os::raw::{c_int, c_longlong, c_uchar, c_uint};

const APULTRA_ERROR: usize = usize::MAX;

extern "C" {
    fn apultra_get_max_compressed_size(input_size: usize) -> usize;

    fn apultra_compress(
        input: *const c_uchar,
        output: *mut c_uchar,
        input_size: usize,
        max_output_size: usize,
        flags: c_uint,
        max_window_size: usize,
        dictionary_size: usize,
        progress: Option<extern "C" fn(c_longlong, c_longlong)>,
        stats: *mut c_int, // pStats is `apultra_stats *`, but we always pass NULL
    ) -> usize;

    fn apultra_decompress(
        input: *const c_uchar,
        output: *mut c_uchar,
        input_size: usize,
        max_output_size: usize,
        dictionary_size: usize,
        flags: c_uint,
    ) -> usize;
}

/// Compress `data` with apultra (optimal aPLib). Output is a stock
/// aPLib stream and can be decoded by any conforming aPLib decoder.
///
/// Panics only on apultra returning an error sentinel, which the
/// library documents as "out of memory" or "buffer too small" — both
/// indicate a host-side bug, not bad input.
pub fn compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        // apultra rejects 0-byte inputs; the caller's empty-chunk path
        // shouldn't reach the codec, but be defensive.
        return Vec::new();
    }
    let max_out = unsafe { apultra_get_max_compressed_size(data.len()) };
    let mut buf = vec![0u8; max_out];
    let written = unsafe {
        apultra_compress(
            data.as_ptr(),
            buf.as_mut_ptr(),
            data.len(),
            buf.len(),
            0,
            0,
            0,
            None,
            std::ptr::null_mut(),
        )
    };
    if written == APULTRA_ERROR {
        panic!(
            "apultra_compress failed (input {} bytes, output buffer {} bytes)",
            data.len(),
            buf.len()
        );
    }
    buf.truncate(written);
    buf
}

/// Decompress an aPLib-formatted stream into a buffer of the exact
/// expected size. Returns an error string on size mismatch or any
/// apultra-reported failure — corrupt archives can land here, so this
/// path must not panic.
pub fn decompress(compressed: &[u8], expected_size: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; expected_size];
    if expected_size == 0 {
        if !compressed.is_empty() {
            return Err(format!(
                "aplib: chunk declares 0 uncompressed bytes but carries {} compressed",
                compressed.len()
            ));
        }
        return Ok(buf);
    }
    let produced = unsafe {
        apultra_decompress(
            compressed.as_ptr(),
            buf.as_mut_ptr(),
            compressed.len(),
            buf.len(),
            0,
            0,
        )
    };
    if produced == APULTRA_ERROR {
        return Err(format!(
            "aplib: apultra_decompress failed (input {} bytes, expected {} bytes out)",
            compressed.len(),
            expected_size
        ));
    }
    if produced != expected_size {
        return Err(format!(
            "aplib: decompressed {} bytes but chunk declared {}",
            produced, expected_size
        ));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn gzip9(data: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::new(9));
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn roundtrip_short() {
        let input = b"hello world, hello world, hello world, hello world.";
        let c = compress(input);
        assert!(!c.is_empty());
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d.as_slice(), input.as_slice());
    }

    #[test]
    fn roundtrip_repeated_block() {
        let mut input = Vec::new();
        for i in 0..4096u16 {
            input.extend_from_slice(&i.to_le_bytes());
            input.extend_from_slice(b"the quick brown fox jumps over the lazy dog\n");
        }
        let c = compress(&input);
        assert!(c.len() < input.len(), "compressed {} >= input {}", c.len(), input.len());
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn roundtrip_zero_padded() {
        let input = vec![0u8; 16384];
        let c = compress(&input);
        let d = decompress(&c, input.len()).unwrap();
        assert_eq!(d, input);
    }

    #[test]
    fn beats_gzip9_on_text() {
        let mut input = String::new();
        for _ in 0..256 {
            input.push_str("Lorem ipsum dolor sit amet, consectetur adipiscing elit. ");
        }
        let bytes = input.as_bytes();
        let apl = compress(bytes);
        let gz = gzip9(bytes);
        assert!(
            apl.len() < gz.len(),
            "aplib {} should beat gzip9 {} on repetitive text",
            apl.len(),
            gz.len()
        );
    }

    #[test]
    fn beats_gzip9_on_zeros() {
        let input = vec![0u8; 32768];
        let apl = compress(&input);
        let gz = gzip9(&input);
        assert!(
            apl.len() < gz.len(),
            "aplib {} should beat gzip9 {} on zero-padded blob",
            apl.len(),
            gz.len()
        );
    }

    #[test]
    fn beats_gzip9_on_executable_like_binary() {
        // Mimics the byte distribution of a small DOS program: a header
        // table, runs of NOP/zero padding, short repeated opcode
        // sequences, and a sparse string pool. This is the regime
        // apultra is tuned for and where it consistently beats gzip.
        let mut input: Vec<u8> = Vec::with_capacity(16384);
        // MZ-style header (mostly zeros with a few magic bytes).
        input.extend_from_slice(b"MZ");
        input.extend(std::iter::repeat_n(0u8, 62));
        // .text-like: short repeated opcode sequences.
        for _ in 0..256 {
            input.extend_from_slice(&[
                0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x10, 0xC7, 0x45, 0xFC, 0x00, 0x00, 0x00, 0x00,
            ]);
            input.extend_from_slice(&[0x90; 3]);
        }
        // .data-like: a string pool with a few repeated short strings.
        for _ in 0..128 {
            input.extend_from_slice(b"error: out of memory\0");
            input.extend_from_slice(b"error: bad magic\0");
        }
        // .bss-like padding.
        input.extend(std::iter::repeat_n(0u8, 2048));
        let apl = compress(&input);
        let gz = gzip9(&input);
        assert!(
            apl.len() < gz.len(),
            "aplib {} should beat gzip9 {} on executable-like binary",
            apl.len(),
            gz.len()
        );
    }

    #[test]
    fn decompress_rejects_size_mismatch() {
        let input = b"some data to compress to test mismatch handling..............";
        let c = compress(input);
        // Tell decompress we expect one more byte than reality — it must error.
        let err = decompress(&c, input.len() + 1).unwrap_err();
        assert!(err.contains("aplib"));
    }
}
