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

    // Watch the whole vendored tree so a `git subtree pull` triggers a
    // rebuild without needing per-file directives.
    println!("cargo:rerun-if-changed={}", vendor.display());
    println!("cargo:rerun-if-changed=build.rs");

    build.compile("apultra");
}
