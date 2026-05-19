//! DKCH archive container. Layout matches PLAN.md §8.
//!
//! Wire format (little-endian everywhere):
//!
//! ```text
//!   [ DKCH archive header ]
//!     "DKCH" magic            4 bytes
//!     version                 u8
//!     algorithm               u8   (0=stored, 1=aplib, 2=lzsa2, 3=lzma)
//!     target_tier             u8   (informational; 0=8086, 3=386, 5=pentium, ...)
//!     flags                   u16  (bit 0 = run_after, bit 1 = encrypted, bit 2 = reproducible)
//!     file_count              u16
//!     total_uncompressed      u32
//!     total_compressed        u32
//!     run_after_offset        u16  (0 if none, relative to archive header start)
//!     header_crc32            u32  (covers every preceding byte of this header)
//!
//!   per file:
//!     name_len                u8
//!     name (NUL-terminated)   name_len bytes (includes the trailing NUL)
//!     attrs                   u8   (FAT attribute byte)
//!     timestamp               u32  (FAT dos_date<<16 | dos_time)
//!     uncompressed_size       u32
//!     chunk_count             u16
//!     per chunk:
//!         compressed_size     u16
//!         uncompressed_size   u16
//!         data                compressed_size bytes
//!     file_crc32              u32  (covers UNCOMPRESSED contents)
//!
//!   [ trailer ]
//!     "DKTR" magic            4 bytes
//!     archive_offset          u32  (offset from file start to the DKCH header)
//! ```
//!
//! The host starts from a prebuilt stub blob (already a complete MZ .EXE,
//! embedded via `include_bytes!`), then appends this archive followed by
//! the trailer — the MZ header is never regenerated. The stub finds the
//! archive by seeking to EOF-8, reading the trailer, and jumping to
//! `archive_offset`.

use std::io::{self, Read, Write};

pub const ARCHIVE_MAGIC: &[u8; 4] = b"DKCH";
pub const TRAILER_MAGIC: &[u8; 4] = b"DKTR";
pub const ARCHIVE_VERSION: u8 = 1;
pub const TRAILER_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Algorithm {
    Stored = 0,
    Aplib = 1,
    Lzsa2 = 2,
    Lzma = 3,
}

impl Algorithm {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Stored),
            1 => Some(Self::Aplib),
            2 => Some(Self::Lzsa2),
            3 => Some(Self::Lzma),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Aplib => "aplib",
            Self::Lzsa2 => "lzsa2",
            Self::Lzma => "lzma",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TargetTier {
    I8086 = 0,
    I286 = 2,
    I386 = 3,
    I486 = 4,
    Pentium = 5,
    PentiumMmx = 6,
    P2 = 7,
    P3 = 8,
}

impl TargetTier {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::I8086),
            2 => Some(Self::I286),
            3 => Some(Self::I386),
            4 => Some(Self::I486),
            5 => Some(Self::Pentium),
            6 => Some(Self::PentiumMmx),
            7 => Some(Self::P2),
            8 => Some(Self::P3),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::I8086 => "8086",
            Self::I286 => "286",
            Self::I386 => "386",
            Self::I486 => "486",
            Self::Pentium => "pentium",
            Self::PentiumMmx => "pentium-mmx",
            Self::P2 => "p2",
            Self::P3 => "p3",
        }
    }
}

pub mod flags {
    pub const RUN_AFTER: u16 = 1 << 0;
    pub const REPRODUCIBLE: u16 = 1 << 2;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    pub uncompressed_size: u16,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEntry {
    /// 8.3 ASCII, NUL-terminated.
    pub name: Vec<u8>,
    pub attrs: u8,
    pub timestamp: u32,
    pub chunks: Vec<Chunk>,
    pub crc32: u32,
}

impl FileEntry {
    pub fn uncompressed_size(&self) -> u32 {
        self.chunks.iter().map(|c| c.uncompressed_size as u32).sum()
    }
    pub fn compressed_size(&self) -> u32 {
        self.chunks.iter().map(|c| c.data.len() as u32).sum()
    }
    pub fn display_name(&self) -> String {
        let name = self.name.split(|b| *b == 0).next().unwrap_or(&self.name);
        String::from_utf8_lossy(name).into_owned()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Archive {
    pub version: u8,
    pub algorithm: Algorithm,
    pub target: TargetTier,
    pub flags: u16,
    pub run_after_offset: u16,
    pub files: Vec<FileEntry>,
}

impl Archive {
    pub fn new(algorithm: Algorithm, target: TargetTier) -> Self {
        Self {
            version: ARCHIVE_VERSION,
            algorithm,
            target,
            flags: 0,
            run_after_offset: 0,
            files: Vec::new(),
        }
    }

    /// Sum of every file's uncompressed and compressed sizes. The
    /// public `pack` path validates the cumulative total fits in u32
    /// *before* calling here, so overflow is a programmer error;
    /// debug builds assert, release builds saturate to keep encoding
    /// total-failure-safe.
    pub fn totals(&self) -> (u32, u32) {
        let mut u: u32 = 0;
        let mut c: u32 = 0;
        for f in &self.files {
            match u.checked_add(f.uncompressed_size()) {
                Some(v) => u = v,
                None => {
                    debug_assert!(false, "totals: uncompressed overflow u32");
                    u = u32::MAX;
                }
            }
            match c.checked_add(f.compressed_size()) {
                Some(v) => c = v,
                None => {
                    debug_assert!(false, "totals: compressed overflow u32");
                    c = u32::MAX;
                }
            }
        }
        (u, c)
    }

    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let header = self.encode_header();
        w.write_all(&header)?;
        for f in &self.files {
            write_file(w, f)?;
        }
        Ok(())
    }

    fn encode_header(&self) -> Vec<u8> {
        let (total_u, total_c) = self.totals();
        let file_count: u16 = self
            .files
            .len()
            .try_into()
            .expect("file_count overflows u16; pack already rejects > 65535 inputs");
        let mut h = Vec::with_capacity(28);
        h.extend_from_slice(ARCHIVE_MAGIC);
        h.push(self.version);
        h.push(self.algorithm as u8);
        h.push(self.target as u8);
        h.extend_from_slice(&self.flags.to_le_bytes());
        h.extend_from_slice(&file_count.to_le_bytes());
        h.extend_from_slice(&total_u.to_le_bytes());
        h.extend_from_slice(&total_c.to_le_bytes());
        h.extend_from_slice(&self.run_after_offset.to_le_bytes());
        let crc = crc32fast::hash(&h);
        h.extend_from_slice(&crc.to_le_bytes());
        h
    }

    pub fn read<R: Read>(r: &mut R) -> Result<Self, ArchiveError> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != ARCHIVE_MAGIC {
            return Err(ArchiveError::BadMagic);
        }
        let mut hdr_rest = [0u8; 17];
        r.read_exact(&mut hdr_rest)?;
        let mut header_bytes = Vec::with_capacity(21);
        header_bytes.extend_from_slice(&magic);
        header_bytes.extend_from_slice(&hdr_rest);
        let computed_crc = crc32fast::hash(&header_bytes);

        let mut crc_buf = [0u8; 4];
        r.read_exact(&mut crc_buf)?;
        let stored_crc = u32::from_le_bytes(crc_buf);
        if stored_crc != computed_crc {
            return Err(ArchiveError::HeaderCrcMismatch {
                expected: stored_crc,
                actual: computed_crc,
            });
        }

        let version = hdr_rest[0];
        if version != ARCHIVE_VERSION {
            return Err(ArchiveError::UnsupportedVersion(version));
        }
        let algorithm =
            Algorithm::from_u8(hdr_rest[1]).ok_or(ArchiveError::UnknownAlgorithm(hdr_rest[1]))?;
        let target =
            TargetTier::from_u8(hdr_rest[2]).ok_or(ArchiveError::UnknownTarget(hdr_rest[2]))?;
        let flags = u16::from_le_bytes([hdr_rest[3], hdr_rest[4]]);
        let file_count = u16::from_le_bytes([hdr_rest[5], hdr_rest[6]]);
        let _total_u = u32::from_le_bytes([hdr_rest[7], hdr_rest[8], hdr_rest[9], hdr_rest[10]]);
        let total_c = u32::from_le_bytes([hdr_rest[11], hdr_rest[12], hdr_rest[13], hdr_rest[14]]);
        let run_after_offset = u16::from_le_bytes([hdr_rest[15], hdr_rest[16]]);

        // Bound parse-time allocations to the archive's own declared
        // total. A hostile producer can still lie about the total, but
        // it can't make us pre-allocate more than the total it claims.
        let mut budget = total_c as u64;
        // Cap the initial allocation. file_count is u16 so the absolute
        // ceiling is 65535 entries (~4 MiB of FileEntry slots), but a
        // hostile header could claim that without any real data behind
        // it. Start small; push() grows naturally if the bytes follow.
        let prealloc = std::cmp::min(file_count as usize, 256);
        let mut files = Vec::with_capacity(prealloc);
        for _ in 0..file_count {
            files.push(read_file(r, &mut budget)?);
        }
        Ok(Self {
            version,
            algorithm,
            target,
            flags,
            run_after_offset,
            files,
        })
    }
}

fn write_file<W: Write>(w: &mut W, f: &FileEntry) -> io::Result<()> {
    let name_len: u8 = f.name.len().try_into().expect("name longer than 255 bytes");
    w.write_all(&[name_len])?;
    w.write_all(&f.name)?;
    w.write_all(&[f.attrs])?;
    w.write_all(&f.timestamp.to_le_bytes())?;
    w.write_all(&f.uncompressed_size().to_le_bytes())?;
    let chunk_count: u16 = f
        .chunks
        .len()
        .try_into()
        .expect("chunk count overflows u16");
    w.write_all(&chunk_count.to_le_bytes())?;
    for c in &f.chunks {
        let csize: u16 = c
            .data
            .len()
            .try_into()
            .expect("chunk compressed size > 65535");
        w.write_all(&csize.to_le_bytes())?;
        w.write_all(&c.uncompressed_size.to_le_bytes())?;
        w.write_all(&c.data)?;
    }
    w.write_all(&f.crc32.to_le_bytes())?;
    Ok(())
}

fn read_file<R: Read>(r: &mut R, budget: &mut u64) -> Result<FileEntry, ArchiveError> {
    let mut b1 = [0u8; 1];
    r.read_exact(&mut b1)?;
    let name_len = b1[0] as usize;
    if name_len < 2 {
        // PLAN.md §8: name is NUL-terminated and the NUL is included in
        // name_len. A 1-byte name would be just the NUL.
        return Err(ArchiveError::EmptyFileName);
    }
    let mut name = vec![0u8; name_len];
    r.read_exact(&mut name)?;
    validate_archive_name(&name)?;

    let mut attrs = [0u8; 1];
    r.read_exact(&mut attrs)?;
    let mut ts = [0u8; 4];
    r.read_exact(&mut ts)?;
    let mut usz = [0u8; 4];
    r.read_exact(&mut usz)?;
    let expected_uncompressed = u32::from_le_bytes(usz);
    let mut cc = [0u8; 2];
    r.read_exact(&mut cc)?;
    let chunk_count = u16::from_le_bytes(cc);

    // chunk_count is u16 (max 65535 ≈ 2 MiB of Chunk slots). Each chunk
    // consumes at least 4 bytes on disk (csize+usize) so the budget
    // bounds it; cap the prealloc anyway so a hostile header can't
    // force a 2 MiB up-front allocation when the body is tiny.
    let max_from_budget = (*budget / 4 + 1) as usize;
    let prealloc = std::cmp::min(chunk_count as usize, std::cmp::min(max_from_budget, 256));
    let mut chunks = Vec::with_capacity(prealloc);
    let mut sum_u: u32 = 0;
    for _ in 0..chunk_count {
        let mut cs = [0u8; 2];
        r.read_exact(&mut cs)?;
        let csize = u16::from_le_bytes(cs) as usize;
        let mut us = [0u8; 2];
        r.read_exact(&mut us)?;
        let usize_u = u16::from_le_bytes(us);
        // Bound the per-chunk allocation by the archive-wide budget the
        // header itself declared. Without this a hostile archive could
        // claim chunk_count = 65535 chunks of csize 65535 and we'd try
        // to allocate ~4 GiB before the read failed.
        if (csize as u64) > *budget {
            return Err(ArchiveError::ArchiveTooLarge {
                declared: *budget,
                kind: "remaining-compressed",
            });
        }
        *budget -= csize as u64;
        let mut data = vec![0u8; csize];
        r.read_exact(&mut data)?;
        // Use checked_add so a malformed archive with many large
        // chunks can't saturate at u32::MAX and pass the
        // sum_u == expected_uncompressed comparison by accident.
        sum_u = sum_u
            .checked_add(usize_u as u32)
            .ok_or(ArchiveError::SizeOverflow)?;
        chunks.push(Chunk {
            uncompressed_size: usize_u,
            data,
        });
    }
    if sum_u != expected_uncompressed {
        return Err(ArchiveError::SizeMismatch {
            file: String::from_utf8_lossy(&name).into_owned(),
            declared: expected_uncompressed,
            from_chunks: sum_u,
        });
    }

    let mut crc = [0u8; 4];
    r.read_exact(&mut crc)?;
    Ok(FileEntry {
        name,
        attrs: attrs[0],
        timestamp: u32::from_le_bytes(ts),
        chunks,
        crc32: u32::from_le_bytes(crc),
    })
}

/// Reject names that aren't NUL-terminated 8.3 ASCII basenames.
/// PLAN.md §8 specifies "8.3 ASCII"; enforce that at parse time so
/// hostile archives can't smuggle non-ASCII or oversize basenames into
/// `unpack`'s extraction paths.
fn validate_archive_name(name: &[u8]) -> Result<(), ArchiveError> {
    if name.last() != Some(&0) {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(name).into_owned(),
            "missing trailing NUL",
        ));
    }
    let body = &name[..name.len() - 1];
    if body.is_empty() {
        return Err(ArchiveError::EmptyFileName);
    }
    // 8.3 = up to 8 chars, optional ".", up to 3 chars = 12 max.
    if body.len() > 12 {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "longer than 8.3",
        ));
    }
    for &b in body {
        match b {
            0 => {
                return Err(ArchiveError::InvalidName(
                    String::from_utf8_lossy(body).into_owned(),
                    "embedded NUL",
                ));
            }
            b if b >= 0x80 => {
                return Err(ArchiveError::InvalidName(
                    String::from_utf8_lossy(body).into_owned(),
                    "non-ASCII byte",
                ));
            }
            b if b < 0x20 || b == 0x7f => {
                return Err(ArchiveError::InvalidName(
                    String::from_utf8_lossy(body).into_owned(),
                    "control character",
                ));
            }
            // `b'.'` is permitted exactly once (the 8.3 separator);
            // handle it before the illegal-set check below so the
            // separator isn't accidentally caught by a future
            // tightening of the FAT illegal set.
            b'.' => {}
            // Mirror the mangler's illegal-byte set (also covers the
            // Windows-illegal characters `* ? " < > |`) so parse-time
            // validation aligns with what `pack` is willing to emit.
            b if crate::name83::ILLEGAL.contains(&b) => {
                return Err(ArchiveError::InvalidName(
                    String::from_utf8_lossy(body).into_owned(),
                    "illegal FAT 8.3 character",
                ));
            }
            _ => {}
        }
    }
    // Enforce the 8.3 stem/ext lengths. Multiple dots are rejected.
    let dot_count = body.iter().filter(|&&b| b == b'.').count();
    if dot_count > 1 {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "more than one dot",
        ));
    }
    if let Some(dot) = body.iter().position(|&b| b == b'.') {
        let stem = &body[..dot];
        let ext = &body[dot + 1..];
        if stem.is_empty() || stem.len() > 8 || ext.len() > 3 {
            return Err(ArchiveError::InvalidName(
                String::from_utf8_lossy(body).into_owned(),
                "stem/ext lengths violate 8.3",
            ));
        }
    } else if body.len() > 8 {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "stem longer than 8",
        ));
    }
    if body == b".." || body == b"." {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "reserved name",
        ));
    }
    // Trailing dot/space — Windows treats `CON.` and `CON ` as the same
    // device. Reject at parse time so hostile archives can't slip past
    // the unpack-side check.
    if body.ends_with(b".") || body.ends_with(b" ") {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "trailing dot or space",
        ));
    }
    // DOS reserved device basenames. Compare on the upper-ASCII stem
    // (everything before the first dot), matching the unpack-side check.
    const DOS_RESERVED: &[&[u8]] = &[
        b"CON", b"PRN", b"AUX", b"NUL", b"COM1", b"COM2", b"COM3", b"COM4", b"COM5", b"COM6",
        b"COM7", b"COM8", b"COM9", b"LPT1", b"LPT2", b"LPT3", b"LPT4", b"LPT5", b"LPT6", b"LPT7",
        b"LPT8", b"LPT9",
    ];
    let stem_lower = body.split(|&b| b == b'.').next().unwrap_or(body);
    let stem_upper: Vec<u8> = stem_lower.iter().map(|b| b.to_ascii_uppercase()).collect();
    if DOS_RESERVED.contains(&stem_upper.as_slice()) {
        return Err(ArchiveError::InvalidName(
            String::from_utf8_lossy(body).into_owned(),
            "reserved DOS device name",
        ));
    }
    Ok(())
}

/// Write the trailer that lets the stub find the archive at runtime.
pub fn write_trailer<W: Write>(w: &mut W, archive_offset: u32) -> io::Result<()> {
    w.write_all(TRAILER_MAGIC)?;
    w.write_all(&archive_offset.to_le_bytes())?;
    Ok(())
}

/// Read and validate the trailer. Returns the archive offset.
pub fn read_trailer(tail: &[u8]) -> Result<u32, ArchiveError> {
    if tail.len() < TRAILER_SIZE {
        return Err(ArchiveError::TrailerTruncated);
    }
    let t = &tail[tail.len() - TRAILER_SIZE..];
    if &t[..4] != TRAILER_MAGIC {
        return Err(ArchiveError::BadTrailerMagic);
    }
    Ok(u32::from_le_bytes([t[4], t[5], t[6], t[7]]))
}

/// Maximum uncompressed bytes per aPLib chunk. aPLib's worst-case
/// expansion is roughly `n + n/8 + 16`, so 16 KiB in → ≤18.4 KiB out, which
/// fits comfortably in the per-chunk u16 `compressed_size` field and lets
/// the 16-bit stub keep both src and dst scratch buffers in its small-model
/// data segment (DS ≤ 64 KiB). Must match `BUF_SIZE` in `stubs/src/stub.c`.
///
/// This is the *maximum* — callers may pick smaller via `--chunk-size`.
/// The stub's `g_src` / `g_buf` BSS buffers are fixed-size at compile
/// time (`APLIB_SRC_SIZE` + `BUF_SIZE`), so smaller chunks do NOT
/// reduce the stub's resident RAM; they only change the archive layout
/// and the actual bytes processed per chunk on disk.
pub const APLIB_CHUNK_INPUT: usize = 16 * 1024;

/// Maximum uncompressed bytes per LZMA chunk. Picked to match
/// `APLIB_CHUNK_INPUT` so a payload packed at the same `--chunk-size`
/// produces the same number of chunks regardless of algorithm. The
/// LZMA stub's `g_lzma_buf` output scratch is sized to match.
pub const LZMA_CHUNK_INPUT: usize = 16 * 1024;

/// Producer-side ceiling on a single LZMA chunk's compressed size. LZMA's
/// worst-case expansion on incompressible data is roughly `n + n/200 +
/// 16`, so a 16 KiB chunk caps at ~16.5 KiB. Plus the 1-byte MicroLZMA
/// props prefix. The 17 KiB ceiling here leaves slack for any edge case
/// the lzma-rust encoder might emit; the on-disk per-chunk u16 size
/// field accepts up to 65535 so this isn't a wire-format constraint.
/// The stub's `g_lzma_src` scratch is sized exactly to this value.
pub const LZMA_MAX_COMPRESSED_CHUNK: usize = 17 * 1024;

/// LZMA dictionary size used at both encode and decode time. The
/// MicroLZMA stream doesn't carry the dict size in-band, so the
/// producer and consumer have to agree on it out of band; we hard-code
/// 16 KiB so the stub's DOS-heap allocation budget is predictable and
/// the encoder doesn't waste cycles building a window the decoder
/// can't use. 16 KiB is at the small end of useful LZMA dictionaries
/// (compression starts to suffer below ~8 KiB) but the trade-off is
/// straightforward: a bigger dict costs DOS conventional RAM at run
/// time on every machine we ship to. Bumping to 32 / 64 KiB is a
/// future change once the DOS-heap allocator is measured in the wild.
pub const LZMA_DICT_SIZE: u32 = 16 * 1024;

/// Producer-side ceiling on a single aPLib chunk's compressed size. The
/// 16-bit stub's `g_src` scratch buffer (`APLIB_SRC_SIZE` in
/// `stubs/src/stub.c`) is sized exactly to this value, so a chunk that
/// passes this check is guaranteed not to trip the stub's runtime
/// `die("aplib csize")` on a real DOS box. Without it, producer-side
/// validation would only catch the much looser u16 overflow (65535) and
/// a future apultra version with worse-than-expected expansion could
/// silently emit archives that refuse to extract.
pub const APLIB_MAX_COMPRESSED_CHUNK: usize = 18_464;

/// Build an aPLib-compressed file entry. Each chunk's uncompressed
/// payload is at most `chunk_size` bytes (which the caller has bounded
/// by `APLIB_CHUNK_INPUT`), so its compressed form fits in the per-chunk
/// u16 size field even in the worst case. CRC32 is computed over the
/// uncompressed bytes, matching the stored path.
pub fn build_aplib_entry(
    name_8_3: &str,
    attrs: u8,
    timestamp: u32,
    data: &[u8],
    chunk_size: usize,
) -> Result<FileEntry, ArchiveError> {
    if !(1..=APLIB_CHUNK_INPUT).contains(&chunk_size) {
        return Err(ArchiveError::InvalidChunkSize {
            algorithm: "aplib",
            given: chunk_size,
            max: APLIB_CHUNK_INPUT,
        });
    }
    // Reject upfront if the file would need > u16::MAX chunks at this
    // chunk_size, so a pathological `--chunk-size 1 some-big-file`
    // doesn't compress tens of thousands of chunks before failing.
    let projected = data.len().div_ceil(chunk_size.max(1));
    if projected > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(projected));
    }
    let crc = crc32fast::hash(data);
    let mut name = name_8_3.as_bytes().to_vec();
    name.push(0);
    let chunks: Vec<Chunk> = if data.is_empty() {
        vec![Chunk {
            uncompressed_size: 0,
            data: Vec::new(),
        }]
    } else {
        let mut out = Vec::with_capacity(data.len().div_ceil(chunk_size));
        for c in data.chunks(chunk_size) {
            let compressed =
                crate::compress::aplib::compress(c).map_err(ArchiveError::AplibCompress)?;
            // Tighter ceiling than u16::MAX: the 16-bit stub's g_src is
            // exactly APLIB_MAX_COMPRESSED_CHUNK bytes. Refuse here so a
            // host-produced archive never trips the runtime check on DOS.
            if compressed.len() > APLIB_MAX_COMPRESSED_CHUNK {
                return Err(ArchiveError::AplibChunkOverflow {
                    uncompressed: c.len(),
                    compressed: compressed.len(),
                });
            }
            out.push(Chunk {
                uncompressed_size: c.len() as u16,
                data: compressed,
            });
        }
        out
    };
    if chunks.len() > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(chunks.len()));
    }
    Ok(FileEntry {
        name,
        attrs,
        timestamp,
        chunks,
        crc32: crc,
    })
}

/// Build a stored-algorithm file entry from raw bytes. Each chunk's
/// size is capped at `chunk_size` (which the caller has bounded so the
/// per-chunk u16 size field never overflows); the per-file chunk count
/// is also u16. The stored stub path streams each chunk through the
/// same `BUF_SIZE` scratch as `copy_bytes`, so the chunk_size value
/// affects the on-disk archive layout but not the stub's RAM use.
pub fn build_stored_entry(
    name_8_3: &str,
    attrs: u8,
    timestamp: u32,
    data: &[u8],
    chunk_size: usize,
) -> Result<FileEntry, ArchiveError> {
    if !(1..=(u16::MAX as usize)).contains(&chunk_size) {
        return Err(ArchiveError::InvalidChunkSize {
            algorithm: "stored",
            given: chunk_size,
            max: u16::MAX as usize,
        });
    }
    // Symmetric early-out to build_aplib_entry: refuse a chunk_size
    // that would project past u16::MAX chunks before allocating any
    // Chunk vector.
    let projected = data.len().div_ceil(chunk_size.max(1));
    if projected > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(projected));
    }
    let crc = crc32fast::hash(data);
    let mut name = name_8_3.as_bytes().to_vec();
    name.push(0);
    let chunks: Vec<Chunk> = if data.is_empty() {
        vec![Chunk {
            uncompressed_size: 0,
            data: Vec::new(),
        }]
    } else {
        data.chunks(chunk_size)
            .map(|c| Chunk {
                uncompressed_size: c.len() as u16,
                data: c.to_vec(),
            })
            .collect()
    };
    if chunks.len() > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(chunks.len()));
    }
    Ok(FileEntry {
        name,
        attrs,
        timestamp,
        chunks,
        crc32: crc,
    })
}

/// Build an LZMA-compressed file entry. Each chunk's uncompressed
/// payload is at most `chunk_size` bytes (bounded by `LZMA_CHUNK_INPUT`
/// at the caller). The compressed stream is xz-embedded's MicroLZMA
/// format: one props byte plus raw LZMA1 range-coded data, no EOS.
///
/// LZMA dict size is fixed at `LZMA_DICT_SIZE`; the encoder and the
/// stub-side decoder both have to agree on it out of band.
pub fn build_lzma_entry(
    name_8_3: &str,
    attrs: u8,
    timestamp: u32,
    data: &[u8],
    chunk_size: usize,
) -> Result<FileEntry, ArchiveError> {
    if !(1..=LZMA_CHUNK_INPUT).contains(&chunk_size) {
        return Err(ArchiveError::InvalidChunkSize {
            algorithm: "lzma",
            given: chunk_size,
            max: LZMA_CHUNK_INPUT,
        });
    }
    let projected = data.len().div_ceil(chunk_size.max(1));
    if projected > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(projected));
    }
    let crc = crc32fast::hash(data);
    let mut name = name_8_3.as_bytes().to_vec();
    name.push(0);
    let chunks: Vec<Chunk> = if data.is_empty() {
        vec![Chunk {
            uncompressed_size: 0,
            data: Vec::new(),
        }]
    } else {
        let mut out = Vec::with_capacity(data.len().div_ceil(chunk_size));
        for c in data.chunks(chunk_size) {
            let compressed = crate::compress::lzma::compress(c, LZMA_DICT_SIZE)
                .map_err(ArchiveError::LzmaCompress)?;
            if compressed.len() > LZMA_MAX_COMPRESSED_CHUNK {
                return Err(ArchiveError::LzmaChunkOverflow {
                    uncompressed: c.len(),
                    compressed: compressed.len(),
                });
            }
            out.push(Chunk {
                uncompressed_size: c.len() as u16,
                data: compressed,
            });
        }
        out
    };
    if chunks.len() > u16::MAX as usize {
        return Err(ArchiveError::TooManyChunks(chunks.len()));
    }
    Ok(FileEntry {
        name,
        attrs,
        timestamp,
        chunks,
        crc32: crc,
    })
}

#[derive(Debug)]
pub enum ArchiveError {
    Io(io::Error),
    BadMagic,
    BadTrailerMagic,
    TrailerTruncated,
    UnsupportedVersion(u8),
    UnknownAlgorithm(u8),
    UnknownTarget(u8),
    HeaderCrcMismatch {
        expected: u32,
        actual: u32,
    },
    EmptyFileName,
    InvalidName(String, &'static str),
    TooManyChunks(usize),
    ArchiveTooLarge {
        declared: u64,
        kind: &'static str,
    },
    SizeOverflow,
    SizeMismatch {
        file: String,
        declared: u32,
        from_chunks: u32,
    },
    AplibChunkOverflow {
        uncompressed: usize,
        compressed: usize,
    },
    AplibCompress(String),
    LzmaChunkOverflow {
        uncompressed: usize,
        compressed: usize,
    },
    LzmaCompress(String),
    InvalidChunkSize {
        algorithm: &'static str,
        given: usize,
        max: usize,
    },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::BadMagic => write!(f, "archive magic mismatch (expected DKCH)"),
            Self::BadTrailerMagic => write!(f, "trailer magic mismatch (expected DKTR)"),
            Self::TrailerTruncated => write!(f, "file too short to contain trailer"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported archive version {v}"),
            Self::UnknownAlgorithm(a) => write!(f, "unknown algorithm byte {a}"),
            Self::UnknownTarget(t) => write!(f, "unknown target tier byte {t}"),
            Self::HeaderCrcMismatch { expected, actual } => write!(
                f,
                "archive header crc mismatch: stored {expected:#010x}, computed {actual:#010x}"
            ),
            Self::EmptyFileName => write!(f, "file entry has zero-length name"),
            Self::InvalidName(name, why) => write!(f, "invalid file name {name:?}: {why}"),
            Self::TooManyChunks(n) => write!(f, "file would need {n} chunks; on-disk chunk_count is u16"),
            Self::ArchiveTooLarge { declared, kind } => {
                write!(f, "archive declares {declared} {kind} bytes, refusing to allocate")
            }
            Self::SizeOverflow => write!(f, "per-chunk uncompressed sizes sum overflow u32"),
            Self::SizeMismatch {
                file,
                declared,
                from_chunks,
            } => write!(
                f,
                "file {file}: declared uncompressed size {declared} != sum of chunks {from_chunks}"
            ),
            Self::AplibChunkOverflow {
                uncompressed,
                compressed,
            } => write!(
                f,
                "aplib chunk: {uncompressed} bytes compressed to {compressed} bytes, overflowing the u16 per-chunk size field"
            ),
            Self::AplibCompress(msg) => write!(f, "{msg}"),
            Self::LzmaChunkOverflow {
                uncompressed,
                compressed,
            } => write!(
                f,
                "lzma chunk: {uncompressed} bytes compressed to {compressed} bytes, exceeding the {} byte stub g_lzma_src ceiling",
                LZMA_MAX_COMPRESSED_CHUNK,
            ),
            Self::LzmaCompress(msg) => write!(f, "{msg}"),
            Self::InvalidChunkSize {
                algorithm,
                given,
                max,
            } => write!(
                f,
                "chunk_size {given} is outside the valid range 1..={max} for algorithm '{algorithm}'"
            ),
        }
    }
}

impl std::error::Error for ArchiveError {}
impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(files: Vec<FileEntry>) {
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        a.flags = flags::REPRODUCIBLE;
        a.files = files;
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let parsed = Archive::read(&mut r).unwrap();
        assert_eq!(a, parsed);
    }

    #[test]
    fn empty_archive_roundtrips() {
        roundtrip(vec![]);
    }

    const STORED_TEST_CHUNK: usize = u16::MAX as usize;

    #[test]
    fn single_file_roundtrips() {
        let data = b"hello dos world";
        let entry = build_stored_entry("HELLO.TXT", 0x20, 0, data, STORED_TEST_CHUNK).unwrap();
        roundtrip(vec![entry]);
    }

    #[test]
    fn zero_byte_file_roundtrips() {
        let entry = build_stored_entry("EMPTY.BIN", 0x20, 0, b"", STORED_TEST_CHUNK).unwrap();
        roundtrip(vec![entry]);
    }

    #[test]
    fn multi_chunk_file_roundtrips() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i & 0xff) as u8).collect();
        let entry = build_stored_entry("BIG.BIN", 0x20, 0, &data, STORED_TEST_CHUNK).unwrap();
        assert!(entry.chunks.len() >= 4, "should split into multiple chunks");
        let total: u32 = entry
            .chunks
            .iter()
            .map(|c| c.uncompressed_size as u32)
            .sum();
        assert_eq!(total as usize, data.len());
        roundtrip(vec![entry]);
    }

    #[test]
    fn stored_respects_smaller_chunk_size() {
        let data: Vec<u8> = (0..32_000u32).map(|i| (i & 0xff) as u8).collect();
        let entry = build_stored_entry("CHUNKED.BIN", 0x20, 0, &data, 4096).unwrap();
        for c in &entry.chunks {
            assert!(c.uncompressed_size as usize <= 4096);
        }
        roundtrip(vec![entry]);
    }

    fn aplib_roundtrip(files: Vec<FileEntry>) {
        let mut a = Archive::new(Algorithm::Aplib, TargetTier::I8086);
        a.flags = flags::REPRODUCIBLE;
        a.files = files;
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let parsed = Archive::read(&mut r).unwrap();
        assert_eq!(a, parsed);
    }

    #[test]
    fn aplib_single_file_roundtrips() {
        let data = b"hello aplib world, hello aplib world, hello aplib world.".repeat(8);
        let entry = build_aplib_entry("HELLO.TXT", 0x20, 0, &data, APLIB_CHUNK_INPUT).unwrap();
        // Compressed should be strictly smaller than uncompressed for a repetitive input.
        let csz: usize = entry.chunks.iter().map(|c| c.data.len()).sum();
        assert!(
            csz < data.len(),
            "expected compression: csz={csz} usz={}",
            data.len()
        );
        assert_eq!(entry.uncompressed_size() as usize, data.len());
        aplib_roundtrip(vec![entry]);
    }

    #[test]
    fn aplib_empty_file_roundtrips() {
        let entry = build_aplib_entry("EMPTY.BIN", 0x20, 0, b"", APLIB_CHUNK_INPUT).unwrap();
        assert_eq!(entry.uncompressed_size(), 0);
        aplib_roundtrip(vec![entry]);
    }

    #[test]
    fn aplib_multi_chunk_file_roundtrips() {
        // Force >1 chunk by exceeding APLIB_CHUNK_INPUT.
        let data: Vec<u8> = (0..(APLIB_CHUNK_INPUT + 1024))
            .map(|i| (i & 0xff) as u8)
            .collect();
        let entry = build_aplib_entry("BIG.BIN", 0x20, 0, &data, APLIB_CHUNK_INPUT).unwrap();
        assert!(entry.chunks.len() >= 2, "should split into multiple chunks");
        let total: u32 = entry
            .chunks
            .iter()
            .map(|c| c.uncompressed_size as u32)
            .sum();
        assert_eq!(total as usize, data.len());
        aplib_roundtrip(vec![entry]);
    }

    fn lzma_roundtrip(files: Vec<FileEntry>) {
        let mut a = Archive::new(Algorithm::Lzma, TargetTier::I386);
        a.flags = flags::REPRODUCIBLE;
        a.files = files;
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let parsed = Archive::read(&mut r).unwrap();
        assert_eq!(a, parsed);
    }

    #[test]
    fn lzma_single_file_roundtrips() {
        let data = b"hello lzma world, hello lzma world, hello lzma world.".repeat(8);
        let entry = build_lzma_entry("HELLO.TXT", 0x20, 0, &data, LZMA_CHUNK_INPUT).unwrap();
        let csz: usize = entry.chunks.iter().map(|c| c.data.len()).sum();
        assert!(
            csz < data.len(),
            "expected compression: csz={csz} usz={}",
            data.len()
        );
        assert_eq!(entry.uncompressed_size() as usize, data.len());
        lzma_roundtrip(vec![entry]);
    }

    #[test]
    fn lzma_empty_file_roundtrips() {
        let entry = build_lzma_entry("EMPTY.BIN", 0x20, 0, b"", LZMA_CHUNK_INPUT).unwrap();
        assert_eq!(entry.uncompressed_size(), 0);
        lzma_roundtrip(vec![entry]);
    }

    #[test]
    fn lzma_multi_chunk_file_roundtrips() {
        let data: Vec<u8> = (0..(LZMA_CHUNK_INPUT + 1024))
            .map(|i| (i & 0xff) as u8)
            .collect();
        let entry = build_lzma_entry("BIG.BIN", 0x20, 0, &data, LZMA_CHUNK_INPUT).unwrap();
        assert!(entry.chunks.len() >= 2, "should split into multiple chunks");
        let total: u32 = entry
            .chunks
            .iter()
            .map(|c| c.uncompressed_size as u32)
            .sum();
        assert_eq!(total as usize, data.len());
        lzma_roundtrip(vec![entry]);
    }

    #[test]
    fn lzma_rejects_chunk_size_above_ceiling() {
        let data = vec![0u8; 32];
        let err = build_lzma_entry("X.BIN", 0x20, 0, &data, LZMA_CHUNK_INPUT + 1).unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidChunkSize { .. }), "got {err:?}");
    }

    #[test]
    fn aplib_respects_smaller_chunk_size() {
        // 8 KiB chunks instead of 16 KiB — host-side knob, stub unchanged.
        let data: Vec<u8> = (0..(APLIB_CHUNK_INPUT + 1024))
            .map(|i| (i & 0xff) as u8)
            .collect();
        let entry = build_aplib_entry("SMALL.BIN", 0x20, 0, &data, 8192).unwrap();
        assert!(
            entry.chunks.len() >= 3,
            "8 KiB chunk should split 17 KiB payload into ≥3 chunks, got {}",
            entry.chunks.len()
        );
        for c in &entry.chunks {
            assert!(c.uncompressed_size as usize <= 8192);
        }
        aplib_roundtrip(vec![entry]);
    }

    #[test]
    fn many_files_roundtrip() {
        let files: Vec<FileEntry> = (0..32u32)
            .map(|i| {
                let name = format!("F{i:04}.TXT");
                let data = format!("contents number {i}\n").into_bytes();
                build_stored_entry(&name, 0x20, 0, &data, STORED_TEST_CHUNK).unwrap()
            })
            .collect();
        roundtrip(files);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut buf = b"XXXX".to_vec();
        buf.extend_from_slice(&[0u8; 32]);
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(matches!(err, ArchiveError::BadMagic));
    }

    #[test]
    fn header_crc_detects_flip() {
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        a.files = vec![build_stored_entry("A.TXT", 0x20, 0, b"a", STORED_TEST_CHUNK).unwrap()];
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        // Flip a bit inside the header (algorithm byte at offset 5).
        buf[5] ^= 0x01;
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(matches!(err, ArchiveError::HeaderCrcMismatch { .. }));
    }

    #[test]
    fn trailer_roundtrip() {
        let mut buf = Vec::new();
        write_trailer(&mut buf, 0xdead_beef).unwrap();
        assert_eq!(read_trailer(&buf).unwrap(), 0xdead_beef);
    }

    #[test]
    fn rejects_non_ascii_name() {
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        // 0xC3 0xA9 = 'é'; the parser must reject non-ASCII bytes.
        a.files = vec![FileEntry {
            name: vec![b'A', 0xC3, 0xA9, 0],
            attrs: 0x20,
            timestamp: 0,
            chunks: vec![Chunk {
                uncompressed_size: 0,
                data: Vec::new(),
            }],
            crc32: crc32fast::hash(&[]),
        }];
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidName(_, _)));
    }

    #[test]
    fn rejects_oversized_8_3_name() {
        // 9-char stem (> 8) with no extension.
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        a.files = vec![FileEntry {
            name: b"AAAAAAAAA\0".to_vec(),
            attrs: 0x20,
            timestamp: 0,
            chunks: vec![Chunk {
                uncompressed_size: 0,
                data: Vec::new(),
            }],
            crc32: crc32fast::hash(&[]),
        }];
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidName(_, _)));
    }

    #[test]
    fn rejects_path_separator_in_name() {
        for bad in [
            &b"../etc\0"[..],
            &b"a/b\0"[..],
            &b"a\\b\0"[..],
            &b"a:b\0"[..],
        ] {
            let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
            a.files = vec![FileEntry {
                name: bad.to_vec(),
                attrs: 0x20,
                timestamp: 0,
                chunks: vec![Chunk {
                    uncompressed_size: 0,
                    data: Vec::new(),
                }],
                crc32: crc32fast::hash(&[]),
            }];
            let mut buf = Vec::new();
            a.write(&mut buf).unwrap();
            let mut r = std::io::Cursor::new(&buf);
            let err = Archive::read(&mut r).unwrap_err();
            assert!(
                matches!(err, ArchiveError::InvalidName(_, _)),
                "expected InvalidName for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn rejects_dos_reserved_and_trailing_dot_or_space() {
        for bad in [
            &b"CON\0"[..],
            &b"con\0"[..],
            &b"NUL.TXT\0"[..],
            &b"COM1.DAT\0"[..],
            &b"lpt9\0"[..],
            &b"PRN\0"[..],
            &b"FILE.\0"[..],
            &b"FILE \0"[..],
        ] {
            let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
            a.files = vec![FileEntry {
                name: bad.to_vec(),
                attrs: 0x20,
                timestamp: 0,
                chunks: vec![Chunk {
                    uncompressed_size: 0,
                    data: Vec::new(),
                }],
                crc32: crc32fast::hash(&[]),
            }];
            let mut buf = Vec::new();
            a.write(&mut buf).unwrap();
            let mut r = std::io::Cursor::new(&buf);
            let err = Archive::read(&mut r).unwrap_err();
            assert!(
                matches!(err, ArchiveError::InvalidName(_, _)),
                "expected InvalidName for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn rejects_del_byte_in_name() {
        // 0x7F (DEL) shouldn't be treated as printable ASCII.
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        a.files = vec![FileEntry {
            name: vec![b'A', 0x7f, b'.', b'T', b'X', b'T', 0],
            attrs: 0x20,
            timestamp: 0,
            chunks: vec![Chunk {
                uncompressed_size: 0,
                data: Vec::new(),
            }],
            crc32: crc32fast::hash(&[]),
        }];
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(
            matches!(err, ArchiveError::InvalidName(_, _)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_name_without_nul_terminator() {
        let mut a = Archive::new(Algorithm::Stored, TargetTier::I8086);
        a.files = vec![FileEntry {
            name: b"NONUL".to_vec(),
            attrs: 0x20,
            timestamp: 0,
            chunks: vec![Chunk {
                uncompressed_size: 0,
                data: Vec::new(),
            }],
            crc32: crc32fast::hash(&[]),
        }];
        let mut buf = Vec::new();
        a.write(&mut buf).unwrap();
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidName(_, _)));
    }

    #[test]
    fn parser_refuses_oversized_csize_vs_declared_total() {
        // Hand-craft an archive whose header declares total_compressed=10
        // but whose single file entry claims a 65535-byte chunk. The
        // parser should refuse to allocate instead of trusting the chunk.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"DKCH");
        buf.push(1); // version
        buf.push(0); // algorithm
        buf.push(0); // target
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&1u16.to_le_bytes()); // file_count
        buf.extend_from_slice(&10u32.to_le_bytes()); // total_u
        buf.extend_from_slice(&10u32.to_le_bytes()); // total_c (10 bytes budget)
        buf.extend_from_slice(&0u16.to_le_bytes()); // run_after
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        // file entry: name "A\0" (len 2), attrs, ts, usize, chunk_count=1
        buf.push(2);
        buf.extend_from_slice(b"A\0");
        buf.push(0x20);
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&10u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        // chunk header: csize=65535 (well above the 10-byte budget)
        buf.extend_from_slice(&65535u16.to_le_bytes());
        buf.extend_from_slice(&10u16.to_le_bytes());
        // (no data — parser should bail before reading)
        let mut r = std::io::Cursor::new(&buf);
        let err = Archive::read(&mut r).unwrap_err();
        assert!(
            matches!(err, ArchiveError::ArchiveTooLarge { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn trailer_bad_magic() {
        let buf = [0u8; 8];
        let err = read_trailer(&buf).unwrap_err();
        assert!(matches!(err, ArchiveError::BadTrailerMagic));
    }
}
