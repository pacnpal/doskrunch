//! Host-side end-to-end roundtrip for `--algo aplib`: pack the fixture
//! set, host-unpack, byte-diff against originals. No DOSBox-X needed.

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
fn pack_then_unpack_is_byte_identical_aplib() {
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
        .args(["--algo", "aplib", "--target", "8086"]);
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
fn aplib_archive_smaller_than_stored() {
    // The whole point of Phase 2: aplib should produce a strictly smaller
    // .EXE than stored on the standard fixture set. (Random.bin alone
    // would expand, but hello/numbers/empty give aplib enough headroom
    // to net-win.) Note that both .exes embed the same stub blob, so the
    // size difference reflects only the archive payload.
    let fixtures = fixtures_dir();
    let inputs: Vec<PathBuf> = ["hello.txt", "numbers.txt", "empty.bin"]
        .iter()
        .map(|n| fixtures.join(n))
        .collect();

    let tmp = tempfile::tempdir().unwrap();
    let stored_exe = tmp.path().join("stored.exe");
    let aplib_exe = tmp.path().join("aplib.exe");

    for (out, algo) in [(&stored_exe, "stored"), (&aplib_exe, "aplib")] {
        let mut cmd = doskrunch();
        cmd.arg("pack")
            .arg(out)
            .args(&inputs)
            .args(["--algo", algo, "--target", "8086"]);
        assert!(cmd.status().unwrap().success(), "pack {algo} failed");
    }

    let stored_size = std::fs::metadata(&stored_exe).unwrap().len();
    let aplib_size = std::fs::metadata(&aplib_exe).unwrap().len();
    assert!(
        aplib_size < stored_size,
        "aplib SFX ({aplib_size}) should be smaller than stored SFX ({stored_size})"
    );
}

#[test]
fn aplib_pack_is_deterministic() {
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
            .args(["--algo", "aplib", "--target", "8086"]);
        assert!(cmd.status().unwrap().success());
    }
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ba, bb, "aplib pack should be reproducible byte-for-byte");
}
