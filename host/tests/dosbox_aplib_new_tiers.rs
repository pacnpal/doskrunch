//! Phase 5 aplib correctness gate across the five new tiers (286, 486,
//! pentium-mmx, p2, p3). Parallel to the existing per-tier
//! `dosbox_aplib_{8086,386,pentium}.rs` gates: pack the fixture set
//! with `--algo aplib --target <tier>`, run under headless DOSBox-X at
//! the matching `cputype=`, assert byte-identical extraction.
//!
//! Collapsed into one file (vs five separate per-tier files) because
//! the surrounding scaffolding is identical and each tier-run is
//! already self-identifying via the loop's `tier` variable in panic
//! messages. The original three Phase-3 per-tier files stay around so
//! their CI history (cputype=8086/386/pentium) remains addressable by
//! filename for back-compat with prior debugging.
//!
//! Catches the same class of bug `dosbox_stored_all_tiers.rs` catches
//! on the stored branch, applied to the aplib branch:
//!   * `aplib_286.bin` exercises `aplib_depack_16.asm` linked into a
//!     wcc -2 stub (vs the wcc -0 stub that ships in `aplib_8086.bin`).
//!   * `aplib_486.bin` exercises `aplib_depack_32.asm` under wcc -4.
//!   * `aplib_pentium-mmx.bin` / `aplib_p2.bin` / `aplib_p3.bin`
//!     exercise `aplib_depack_p5.asm` under wcc -5 (pmmx) and wcc -6
//!     (p2, p3). The p6 codegen for the surrounding C housekeeping is
//!     the load-bearing difference at p2 / p3 — the depacker .asm is
//!     `cpu pentium`, a strict subset of P6.
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
fn extracts_aplib_fixtures_across_new_tiers() {
    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();

    let bin = env!("CARGO_BIN_EXE_doskrunch");

    // (--target value, DOSBox-X cputype value) per new tier. Spellings
    // validated against dosbox-x 2026.05.02; see the comment in
    // dosbox_stored_all_tiers.rs for the full reasoning around the p2
    // / p3 names.
    let tiers: &[(&str, &str)] = &[
        ("286", "286"),
        ("486", "486"),
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
            .args(["--algo", "aplib", "--target", tier])
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
            Err(WaitError::Wait(e)) => {
                panic!("waiting on dosbox-x failed: {e} (tier {tier}); child was killed")
            }
        };
        assert!(
            dosbox_status.success(),
            "dosbox-x exited non-zero (tier {tier}): {dosbox_status:?}",
        );

        for fixture in fixtures() {
            let original = fs::read(fixtures_dir.join(fixture))
                .unwrap_or_else(|e| panic!("read fixture {fixture}: {e}"));
            let extracted_name = fixture.to_ascii_uppercase();
            let extracted =
                locate_case_insensitive(work_path, &extracted_name).unwrap_or_else(|| {
                    panic!("missing extracted file {extracted_name} on tier {tier}")
                });
            let body = fs::read(&extracted).unwrap_or_else(|e| {
                panic!("read extracted {} (tier {tier}): {e}", extracted.display())
            });
            assert_eq!(
                body,
                original,
                "tier {tier}: extracted {} differs from fixture {}",
                extracted.display(),
                fixture
            );
        }
    }
}
