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
    unpack
        .arg("unpack")
        .arg(&exe)
        .arg("-d")
        .arg(&extracted);
    let status = unpack.status().unwrap();
    assert!(status.success(), "unpack failed");

    for src in &inputs {
        let name = src.file_name().unwrap().to_str().unwrap().to_uppercase();
        let dst = extracted.join(&name);
        let original = std::fs::read(src).unwrap();
        let actual = std::fs::read(&dst)
            .unwrap_or_else(|e| panic!("missing {}: {}", dst.display(), e));
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
    assert_eq!(ba, bb, "argv order should not affect reproducible-mode output");
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
