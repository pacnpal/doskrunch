//! Phase 5 LZMA correctness gate across every shipped LZMA tier.
//!
//! PLAN.md §4: LZMA is 386+ only. Six tiers ship: 386, 486, pentium,
//! pentium-mmx, p2, p3. Each has its own per-tier blob built with the
//! matching wcc CPU flag; they all link the same vendor/xz-embedded
//! MicroLZMA decoder.
//!
//! For each tier: pack the fixture set with `--algo lzma --target
//! <tier>`, run the resulting SFX under headless DOSBox-X at the
//! matching `cputype=`, assert byte-identical extraction. Same shape
//! as `dosbox_aplib_new_tiers.rs` and `dosbox_stored_all_tiers.rs`.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{fixtures, locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(180);

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_lzma_fixtures_across_all_lzma_tiers() {
    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();

    let bin = env!("CARGO_BIN_EXE_doskrunch");

    // (--target value, DOSBox-X cputype value) per LZMA-eligible tier.
    let tiers: &[(&str, &str)] = &[
        ("386", "386"),
        ("486", "486"),
        ("pentium", "pentium"),
        ("pentium-mmx", "pentium_mmx"),
        ("p2", "pentium_ii"),
        ("p3", "pentium_iii"),
    ];

    for (tier, cputype) in tiers {
        let work = tempfile::tempdir().expect("create tempdir");
        let work_path = work.path();

        let sfx_path = work_path.join("OUT.EXE");
        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .args(&inputs)
            .args(["--algo", "lzma", "--target", tier])
            .status()
            .expect("spawn doskrunch pack");
        assert!(
            status.success(),
            "doskrunch pack failed for tier {tier}: {status:?}"
        );

        let conf_path = work_path.join("dosbox.conf");
        fs::write(
            &conf_path,
            format!(
                concat!(
                    "[cpu]\n",
                    "cputype={cputype}\n",
                    "core=normal\n",
                    "cycles=max\n",
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
                "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} (lzma tier {tier}); child was killed"
            ),
            Err(WaitError::Wait(e)) => {
                panic!("waiting on dosbox-x failed: {e} (lzma tier {tier}); child was killed")
            }
        };
        assert!(
            dosbox_status.success(),
            "dosbox-x exited non-zero (lzma tier {tier}): {dosbox_status:?}",
        );

        for fixture in fixtures() {
            let original = fs::read(fixtures_dir.join(fixture))
                .unwrap_or_else(|e| panic!("read fixture {fixture}: {e}"));
            let extracted_name = fixture.to_ascii_uppercase();
            let extracted =
                locate_case_insensitive(work_path, &extracted_name).unwrap_or_else(|| {
                    panic!("missing extracted file {extracted_name} on lzma tier {tier}")
                });
            let body = fs::read(&extracted).unwrap_or_else(|e| {
                panic!("read extracted {} (lzma tier {tier}): {e}", extracted.display())
            });
            assert_eq!(
                body,
                original,
                "lzma tier {tier}: extracted {} differs from fixture {}",
                extracted.display(),
                fixture
            );
        }
    }
}
