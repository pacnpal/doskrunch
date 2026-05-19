//! Host-side end-to-end roundtrip: pack a fixture set, unpack it, diff.
//!
//! Phase 1's verify gate. Doesn't run DOS — that's the DOSBox-X integration
//! test in CI.

use std::path::PathBuf;
use std::process::Command;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
}

fn doskrunch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_doskrunch"))
}

#[test]
fn pack_then_unpack_is_byte_identical() {
    let fixtures = fixtures_dir();
    let inputs: Vec<PathBuf> = ["hello.txt", "numbers.txt", "random.bin", "empty.bin"]
        .iter()
        .map(|n| fixtures.join(n))
        .collect();

    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("out.exe");
    let extracted = tmp.path().join("extracted");

    let mut pack = doskrunch();
    pack.arg("pack")
        .arg(&exe)
        .args(&inputs)
        .args(["--algo", "stored", "--target", "8086"]);
    let status = pack.status().unwrap();
    assert!(status.success(), "pack failed");

    let mut unpack = doskrunch();
    unpack.arg("unpack").arg(&exe).arg("-d").arg(&extracted);
    let status = unpack.status().unwrap();
    assert!(status.success(), "unpack failed");

    for src in &inputs {
        let name = src.file_name().unwrap().to_str().unwrap().to_uppercase();
        let dst = extracted.join(&name);
        let original = std::fs::read(src).unwrap();
        let actual =
            std::fs::read(&dst).unwrap_or_else(|e| panic!("missing {}: {}", dst.display(), e));
        assert_eq!(original, actual, "content mismatch for {name}");
    }
}

#[test]
fn pack_is_deterministic_in_reproducible_mode() {
    let fixtures = fixtures_dir();
    let inputs: Vec<PathBuf> = ["hello.txt", "numbers.txt"]
        .iter()
        .map(|n| fixtures.join(n))
        .collect();
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.exe");
    let b = tmp.path().join("b.exe");
    for out in [&a, &b] {
        let mut cmd = doskrunch();
        cmd.arg("pack")
            .arg(out)
            .args(&inputs)
            .args(["--algo", "stored", "--target", "8086"]);
        assert!(cmd.status().unwrap().success());
    }
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ba, bb, "reproducible mode should produce identical bytes");
}

#[test]
fn pack_output_independent_of_argv_order() {
    let fixtures = fixtures_dir();
    let forward: Vec<PathBuf> = ["empty.bin", "hello.txt", "numbers.txt", "random.bin"]
        .iter()
        .map(|n| fixtures.join(n))
        .collect();
    let reversed: Vec<PathBuf> = forward.iter().rev().cloned().collect();
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("forward.exe");
    let b = tmp.path().join("reversed.exe");
    for (out, ins) in [(&a, &forward), (&b, &reversed)] {
        let mut cmd = doskrunch();
        cmd.arg("pack")
            .arg(out)
            .args(ins)
            .args(["--algo", "stored", "--target", "8086"]);
        assert!(cmd.status().unwrap().success());
    }
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(
        ba, bb,
        "argv order should not affect reproducible-mode output"
    );
}

#[test]
fn pack_walks_directory_recursively() {
    // Build a small tree:
    //   src/a.txt
    //   src/inner/b.txt
    //   src/inner/c.txt
    // Pack with the directory as the only input, then unpack and confirm
    // every regular file under src/ landed in the extracted directory
    // (flat — Phase 4 doesn't recreate subdirectories at extract time).
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("inner")).unwrap();
    std::fs::write(src.join("a.txt"), b"alpha-contents").unwrap();
    std::fs::write(src.join("inner").join("b.txt"), b"bravo-bytes").unwrap();
    std::fs::write(src.join("inner").join("c.txt"), b"charlie-data").unwrap();

    let exe = tmp.path().join("dir.exe");
    let mut pack = doskrunch();
    pack.arg("pack")
        .arg(&exe)
        .arg(&src)
        .args(["--algo", "stored", "--target", "8086"]);
    let status = pack.status().unwrap();
    assert!(status.success(), "pack failed for directory input");

    let extracted = tmp.path().join("out");
    let mut unpack = doskrunch();
    unpack.arg("unpack").arg(&exe).arg("-d").arg(&extracted);
    let status = unpack.status().unwrap();
    assert!(status.success(), "unpack failed");

    let read = |name: &str| std::fs::read(extracted.join(name)).unwrap_or_default();
    assert_eq!(read("A.TXT"), b"alpha-contents");
    assert_eq!(read("B.TXT"), b"bravo-bytes");
    assert_eq!(read("C.TXT"), b"charlie-data");
}

#[test]
fn directory_pack_is_deterministic_across_two_invocations() {
    // Same tree packed twice must produce identical bytes. Exercises
    // both the walk-ordering and the reproducible-mode sort.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("z").join("inner")).unwrap();
    std::fs::create_dir_all(src.join("a")).unwrap();
    // Deliberate non-alphabetical creation order — output should still
    // be deterministic because pack sorts by mangled name.
    std::fs::write(src.join("z").join("inner").join("zz.txt"), b"zz").unwrap();
    std::fs::write(src.join("a").join("aa.txt"), b"aa").unwrap();
    std::fs::write(src.join("mid.txt"), b"mid").unwrap();

    let a = tmp.path().join("a.exe");
    let b = tmp.path().join("b.exe");
    for out in [&a, &b] {
        let mut cmd = doskrunch();
        cmd.arg("pack").arg(out).arg(&src);
        assert!(cmd.status().unwrap().success());
    }
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ba, bb, "directory walk must be reproducible");
}

#[test]
fn chunk_size_flag_respected_end_to_end() {
    // Pack with a tiny chunk size and verify the produced archive
    // actually uses chunks of at most that size — re-open the archive
    // via doskrunch::unpack::load_archive and inspect each chunk's
    // declared uncompressed_size. Without this check, a CLI bug that
    // silently ignored --chunk-size and produced 16 KiB chunks would
    // still pass an unpack-and-diff roundtrip.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("payload.bin");
    // 40_000 bytes at 4 KiB chunks = 10 chunks minimum; at the
    // default 16 KiB chunks it'd be only 3, so chunk count alone
    // also distinguishes the two cases.
    let bytes: Vec<u8> = (0..40_000u32).map(|i| (i & 0xff) as u8).collect();
    std::fs::write(&src, &bytes).unwrap();

    let exe = tmp.path().join("small.exe");
    let mut pack = doskrunch();
    pack.arg("pack").arg(&exe).arg(&src).args([
        "--algo",
        "aplib",
        "--target",
        "8086",
        "--chunk-size",
        "4096",
    ]);
    let status = pack.status().unwrap();
    assert!(status.success(), "pack with --chunk-size failed");

    let archive = doskrunch::unpack::load_archive(&exe).expect("load archive");
    assert_eq!(archive.files.len(), 1);
    let entry = &archive.files[0];
    assert!(
        entry.chunks.len() >= 10,
        "40_000 B at chunk_size=4096 should produce ≥10 chunks; got {}",
        entry.chunks.len()
    );
    for c in &entry.chunks {
        assert!(
            c.uncompressed_size as usize <= 4096,
            "chunk uncompressed_size {} exceeds --chunk-size 4096; CLI flag not threaded through",
            c.uncompressed_size,
        );
    }

    let extracted = tmp.path().join("out");
    let mut unpack = doskrunch();
    unpack.arg("unpack").arg(&exe).arg("-d").arg(&extracted);
    assert!(unpack.status().unwrap().success(), "unpack failed");

    let got = std::fs::read(extracted.join("PAYLOAD.BIN")).unwrap();
    assert_eq!(got, bytes, "small-chunk roundtrip mismatch");
}

#[test]
fn stored_max_chunk_size_roundtrips_via_host_unpack() {
    // Host-side sanity check that --chunk-size 65535 produces the
    // requested 4-chunk archive layout (200_000 B / 65535 ≈ 4 chunks)
    // and round-trips byte-identical through `doskrunch unpack`.
    // This does NOT exercise the DOS stub's `copy_bytes` loop — that
    // happens in `dosbox_stored_max_chunk.rs`, which runs the same
    // archive under DOSBox-X. The aplib large-payload DOSBox-X gates
    // (500 KiB, 2 MiB) cover a different stub path: aplib chunks
    // flow through `g_src` → `aplib_depack` → `g_buf` → single
    // write, not through the `copy_bytes` chunk-streaming loop.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("payload.bin");
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i & 0xff) as u8).collect();
    std::fs::write(&src, &bytes).unwrap();

    let exe = tmp.path().join("stored_big.exe");
    let mut pack = doskrunch();
    pack.arg("pack").arg(&exe).arg(&src).args([
        "--algo",
        "stored",
        "--target",
        "8086",
        "--chunk-size",
        "65535",
    ]);
    assert!(pack.status().unwrap().success(), "pack failed");

    let archive = doskrunch::unpack::load_archive(&exe).expect("load archive");
    assert_eq!(archive.files.len(), 1);
    let entry = &archive.files[0];
    // 200_000 bytes / 65535 = 4 chunks (3 × 65535 + 1 × 3395).
    assert_eq!(entry.chunks.len(), 4);
    for c in entry.chunks.iter().take(3) {
        assert_eq!(c.uncompressed_size, 65535, "first 3 chunks at max size");
    }

    let extracted = tmp.path().join("out");
    let mut unpack = doskrunch();
    unpack.arg("unpack").arg(&exe).arg("-d").arg(&extracted);
    assert!(unpack.status().unwrap().success(), "unpack failed");
    let got = std::fs::read(extracted.join("PAYLOAD.BIN")).unwrap();
    assert_eq!(got, bytes, "stored 65535-byte-chunk roundtrip mismatch");
}

#[test]
fn chunk_size_above_stub_budget_for_aplib_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"x").unwrap();
    let exe = tmp.path().join("o.exe");
    let mut pack = doskrunch();
    pack.arg("pack").arg(&exe).arg(&src).args([
        "--algo",
        "aplib",
        "--target",
        "8086",
        "--chunk-size",
        "65535",
    ]);
    let out = pack.output().unwrap();
    assert!(!out.status.success(), "should reject oversize chunk_size");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("chunk-size") || stderr.contains("chunk_size"));
}

#[test]
fn lzsa2_chunk_size_above_ceiling_is_rejected() {
    // Phase 6 ships LZSA2 with its own chunk-size ceiling
    // (LZSA2_CHUNK_INPUT). The prior `deferred_algorithm_takes_
    // precedence_over_chunk_size_validation` gate covered the case
    // where lzsa2 was deferred; with all four algorithms shipped
    // there's no more deferred-precedence logic to gate. This test
    // is its replacement: prove the LZSA2 chunk-size validation
    // works end-to-end through the CLI.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"x").unwrap();
    let exe = tmp.path().join("o.exe");

    let mut pack = doskrunch();
    pack.arg("pack").arg(&exe).arg(&src).args([
        "--algo",
        "lzsa2",
        "--target",
        "8086",
        "--chunk-size",
        "99999",
    ]);
    let out = pack.output().unwrap();
    assert!(!out.status.success(), "lzsa2 with oversize chunk_size should bail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("chunk-size") || stderr.contains("chunk_size"),
        "expected chunk-size error, got: {stderr}"
    );
}

#[test]
fn run_after_roundtrips_through_pack_and_inspect() {
    // Pack with --run-after, then run `inspect` and confirm the
    // command + flag both show up. End-to-end exercise of the host
    // pack -> archive serialization -> read -> inspect chain.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"hello run-after").unwrap();
    let exe = tmp.path().join("o.exe");
    let mut pack = doskrunch();
    pack.arg("pack")
        .arg(&exe)
        .arg(&src)
        .args(["--algo", "aplib", "--target", "8086", "--run-after", "MY.EXE /Q"]);
    let out = pack.output().unwrap();
    assert!(
        out.status.success(),
        "pack with --run-after failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut inspect = doskrunch();
    inspect.arg("inspect").arg(&exe);
    let out = inspect.output().unwrap();
    assert!(out.status.success(), "inspect failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("run-after") && stdout.contains("MY.EXE /Q"),
        "inspect output missing run-after info: {stdout}"
    );
}

#[test]
fn run_after_rejects_invalid_input_at_the_cli_layer() {
    // Non-printable byte inside the command -> bail at pack time
    // with a clear error. Mirrors archive::set_run_after's
    // validation; this test gates the host all the way through the
    // CLI layer.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"x").unwrap();
    let exe = tmp.path().join("o.exe");
    let mut pack = doskrunch();
    pack.arg("pack")
        .arg(&exe)
        .arg(&src)
        .args(["--algo", "aplib", "--target", "8086", "--run-after", "BAD\tCMD"]);
    let out = pack.output().unwrap();
    assert!(!out.status.success(), "pack should reject tab in --run-after");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("run-after"),
        "expected run-after error, got: {stderr}"
    );
}

#[test]
fn lzma_rejected_on_8086_and_286_at_the_cli_layer() {
    // Phase 5 ships LZMA on 386+. The 8086 / 286 tiers stay rejected
    // by pack() because the decoder's 32-bit math doesn't fit those
    // CPUs. Test both rejected tiers end-to-end through the CLI.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"x").unwrap();
    let exe = tmp.path().join("o.exe");
    for target in &["8086", "286"] {
        let mut pack = doskrunch();
        pack.arg("pack").arg(&exe).arg(&src).args([
            "--algo", "lzma", "--target", target,
        ]);
        let out = pack.output().unwrap();
        assert!(
            !out.status.success(),
            "pack should reject lzma on tier {target}"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("lzma") && stderr.contains("386 or higher"),
            "tier {target}: expected lzma + 386 message, got: {stderr}"
        );
    }
}

#[test]
fn chunk_size_above_u16_for_stored_is_rejected() {
    // Symmetric to the aplib-rejection case: stored allows up to
    // u16::MAX (65535), so 65536 must bail at the CLI boundary with
    // the same wording. Guards the algorithm-specific validation
    // branch that the aplib test doesn't cover.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.bin");
    std::fs::write(&src, b"x").unwrap();
    let exe = tmp.path().join("o.exe");
    let mut pack = doskrunch();
    pack.arg("pack").arg(&exe).arg(&src).args([
        "--algo",
        "stored",
        "--target",
        "8086",
        "--chunk-size",
        "65536",
    ]);
    let out = pack.output().unwrap();
    assert!(
        !out.status.success(),
        "stored should reject chunk_size > u16::MAX"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("chunk-size") || stderr.contains("chunk_size"));
    assert!(stderr.contains("65535") || stderr.contains("stored"));
}

#[test]
fn inspect_runs() {
    let fixtures = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("o.exe");
    let mut pack = doskrunch();
    pack.arg("pack")
        .arg(&exe)
        .arg(fixtures.join("hello.txt"))
        .args(["--algo", "stored", "--target", "8086"]);
    assert!(pack.status().unwrap().success());
    let mut ins = doskrunch();
    let out = ins.arg("inspect").arg(&exe).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stored"));
    assert!(stdout.contains("8086"));
    assert!(stdout.contains("HELLO.TXT"));
}
