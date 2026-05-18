//! Phase 3 stored-path correctness gate across every shipped tier.
//!
//! `host/src/stubs.rs` routes both `Algorithm::Stored` and
//! `Algorithm::Aplib` to the same per-tier blob, and the stub's main
//! loop dispatches at runtime on the archive's algorithm byte:
//!
//!   * `algo == 0` (stored) — streaming `copy_bytes` through `g_buf`,
//!     no depacker involved.
//!   * `algo == 1` (aplib)  — read full compressed chunk into `g_src`,
//!     call `aplib_depack`, write `g_buf`.
//!
//! These are independent code paths. The existing `dosbox_aplib_*`
//! gates exercise the aplib branch; the Phase 1 `dosbox_8086.rs` smoke
//! test originally covered stored, but Phase 2 flipped the host's
//! `--algo` default to aplib, so `dosbox_8086.rs` now also takes the
//! aplib branch.
//!
//! This file fills the gap: explicit `--algo stored` packs for each of
//! the three shipped tiers, run under headless DOSBox-X at the matching
//! `cputype=`, asserting byte-identical extraction. Catches a class of
//! bug Phase 3 would otherwise miss — Watcom's `-3` / `-5` codegen for
//! the C housekeeping in the stored branch hasn't been runtime-verified
//! on the new tiers.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(120);

fn fixtures() -> &'static [&'static str] {
    &["hello.txt", "numbers.txt", "random.bin", "empty.bin"]
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_stored_fixtures_across_all_shipped_tiers() {
    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();

    let bin = env!("CARGO_BIN_EXE_doskrunch");

    for (tier, cputype) in &[("8086", "8086"), ("386", "386"), ("pentium", "pentium")] {
        let work = tempfile::tempdir().expect("create tempdir");
        let work_path = work.path();

        let sfx_path = work_path.join("OUT.EXE");
        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .args(&inputs)
            .args(["--algo", "stored", "--target", tier])
            .status()
            .expect("spawn doskrunch pack");
        assert!(status.success(), "doskrunch pack failed for tier {tier}: {status:?}");

        let conf_path = work_path.join("dosbox.conf");
        fs::write(
            &conf_path,
            format!(
                concat!(
                    "[cpu]\n",
                    "cputype={cputype}\n",
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
                cputype = cputype,
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
            .expect("spawn dosbox-x");
        let dosbox_status = match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
            Ok(s) => s,
            Err(WaitError::Timeout) => panic!(
                "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} (tier {tier}); child was killed"
            ),
            Err(WaitError::Wait(e)) => panic!(
                "waiting on dosbox-x failed: {e} (tier {tier}); child was killed"
            ),
        };
        assert!(
            dosbox_status.success(),
            "dosbox-x exited non-zero (tier {tier}): {dosbox_status:?}",
        );

        for fixture in fixtures() {
            let original = fs::read(fixtures_dir.join(fixture))
                .unwrap_or_else(|e| panic!("read fixture {fixture}: {e}"));
            let extracted_name = fixture.to_ascii_uppercase();
            let extracted = locate_case_insensitive(work_path, &extracted_name)
                .unwrap_or_else(|| panic!(
                    "missing extracted file {extracted_name} on tier {tier}"
                ));
            let body = fs::read(&extracted)
                .unwrap_or_else(|e| panic!("read extracted {} (tier {tier}): {e}", extracted.display()));
            assert_eq!(
                body, original,
                "tier {tier}: extracted {} differs from fixture {}",
                extracted.display(),
                fixture
            );
        }
    }
}

