//! Phase 3 verify gate: pack the fixture set with `--algo aplib --target pentium`,
//! run the SFX under headless DOSBox-X at `cputype=pentium`, and confirm
//! extracted files match the originals byte-for-byte.
//!
//! Parallel to `dosbox_aplib_386.rs` — same fixture set, same timeout
//! pattern, same headless config. Differences: `--target pentium` on pack
//! (selects `stubs/blobs/aplib_pentium.bin`, the wcc -5 + speed-optimized
//! 32-bit depacker stub) and `cputype=pentium` on the emulator.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked.
//! CI's `dosbox-x-integration` job runs it via `cargo test -- --ignored`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{fixtures, locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(120);

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_aplib_fixtures_under_pentium_cputype() {
    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");

    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    let sfx_path = work_path.join("OUT.EXE");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();

    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .args(&inputs)
        .args(["--algo", "aplib", "--target", "pentium"])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "doskrunch pack failed: {status:?}");

    let conf_path = work_path.join("dosbox.conf");
    fs::write(
        &conf_path,
        format!(
            concat!(
                "[cpu]\n",
                "cputype=pentium\n",
                "core=normal\n",
                "[dosbox]\n",
                "memsize=4\n",
                "[sdl]\n",
                "output=surface\n",
                "[autoexec]\n",
                "mount c \"{mount}\"\n",
                "c:\n",
                "OUT.EXE\n",
                "exit\n",
            ),
            mount = work_path.display(),
        ),
    )
    .expect("write dosbox.conf");

    let mut dosbox = Command::new("dosbox-x")
        .arg("-conf")
        .arg(&conf_path)
        .arg("-exit")
        .arg("-nogui")
        .env("SDL_VIDEODRIVER", "dummy")
        .spawn()
        .expect("spawn dosbox-x (is it installed?)");
    let dosbox_status = match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
        Ok(status) => status,
        Err(WaitError::Timeout) => {
            panic!("dosbox-x did not exit within {DOSBOX_TIMEOUT:?}; child was killed")
        }
        Err(WaitError::Wait(e)) => panic!("waiting on dosbox-x failed: {e}; child was killed"),
    };
    assert!(
        dosbox_status.success(),
        "dosbox-x exited non-zero: {dosbox_status:?}",
    );

    for fixture in fixtures() {
        let original = fs::read(fixtures_dir.join(fixture))
            .unwrap_or_else(|e| panic!("read fixture {fixture}: {e}"));
        let extracted_name = fixture.to_ascii_uppercase();
        let extracted = locate_case_insensitive(work_path, &extracted_name)
            .unwrap_or_else(|| panic!("missing extracted file: {extracted_name}"));
        let body = fs::read(&extracted)
            .unwrap_or_else(|e| panic!("read extracted {}: {e}", extracted.display()));
        assert_eq!(
            body,
            original,
            "extracted {} differs from fixture {}",
            extracted.display(),
            fixture
        );
    }
}
