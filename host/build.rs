//! Compile the vendored apultra C library (compressor + decompressor)
//! into a static archive linked into the doskrunch binary.
//!
//! Sources mirror the file list in `vendor/apultra/Makefile`'s LIBOBJS,
//! excluding `src/apultra.c` (the upstream CLI front-end). The host
//! talks to the library through `apultra_compress` / `apultra_decompress`
//! / `apultra_get_max_compressed_size` declared in
//! `vendor/apultra/src/{shrink.h,expand.h}`.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = root.parent().expect("host/ has a parent");
    let vendor = workspace.join("vendor/apultra");
    let src = vendor.join("src");
    let divsufsort_lib = src.join("libdivsufsort/lib");
    let divsufsort_inc = src.join("libdivsufsort/include");

    let sources = [
        src.join("expand.c"),
        src.join("shrink.c"),
        src.join("matchfinder.c"),
        divsufsort_lib.join("divsufsort.c"),
        divsufsort_lib.join("divsufsort_utils.c"),
        divsufsort_lib.join("sssort.c"),
        divsufsort_lib.join("trsort.c"),
    ];

    let mut build = cc::Build::new();
    build
        .files(&sources)
        .include(&src)
        .include(&divsufsort_inc)
        .define("NDEBUG", None)
        .warnings(false);

    // Only watch the C library source dir. The rest of the vendored
    // tree (vendor/apultra/asm/, VS2017/, README, etc.) doesn't affect
    // the static lib we compile here; an unrelated subtree pull
    // shouldn't force a full apultra rebuild.
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=build.rs");
    // Cross-compile / toolchain-swap robustness: cc-rs reads these to
    // decide which compiler and flags to use, so changing them should
    // force the static archive to be rebuilt.
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=CFLAGS");
    println!("cargo:rerun-if-env-changed=AR");

    build.compile("apultra");

    // -- xz-embedded MicroLZMA decoder (host-side) -----------------------
    //
    // Vendored under vendor/xz-embedded; Linux-kernel source layout. We
    // compile the minimal decoder-only file set:
    //   * xz_crc32.c       — CRC32 table init (xz-embedded internal)
    //   * xz_dec_lzma2.c   — LZMA2 + MicroLZMA decoder core
    //
    // xz_dec_stream.c (.xz container parser) and xz_dec_bcj.c (BCJ
    // filters) are NOT compiled: the MicroLZMA APIs
    // (xz_dec_microlzma_*) live entirely in xz_dec_lzma2.c, and
    // doskrunch's archive framing carries the LZMA stream as raw
    // MicroLZMA blocks with per-chunk uncomp/comp sizes already in
    // the DKCH header. The .xz container would add ~32-48 bytes per
    // chunk for redundant framing.
    let xz_root = workspace.join("vendor/xz-embedded");
    let xz_inc = xz_root.join("linux/include/linux");
    let xz_lib = xz_root.join("linux/lib/xz");
    let xz_cfg = xz_root.join("userspace");

    let xz_sources = [
        xz_lib.join("xz_crc32.c"),
        xz_lib.join("xz_dec_lzma2.c"),
    ];

    let mut xz_build = cc::Build::new();
    xz_build
        .files(&xz_sources)
        .include(&xz_inc)
        .include(&xz_lib)
        .include(&xz_cfg)
        // Wire in the MicroLZMA decoder. xz_dec_lzma2.c gates the
        // xz_dec_microlzma_* API behind XZ_DEC_MICROLZMA so the kernel
        // build can omit it; we need it exposed for both the host's
        // round-trip tests and the stub's chunk decode path.
        .define("XZ_DEC_MICROLZMA", None)
        // Quiet a noisy `fallthrough` macro redefinition warning under
        // newer compilers; the macro definition in xz_config.h is the
        // right one for the kernel-style C99 fallthrough attribute.
        .warnings(false);
    println!("cargo:rerun-if-changed={}", xz_lib.display());
    // xz_inc holds the public headers (xz.h, xz_private.h, etc) that
    // both xz_dec_lzma2.c and our FFI bindings include. Header-only
    // changes must trigger a rebuild of the static lib too.
    println!("cargo:rerun-if-changed={}", xz_inc.display());
    println!("cargo:rerun-if-changed={}", xz_cfg.display());
    xz_build.compile("xz_embedded");
}
