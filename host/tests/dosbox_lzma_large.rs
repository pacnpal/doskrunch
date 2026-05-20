//! Phase 5 multi-chunk LZMA correctness gate: pack a 500 KiB synthetic
//! mixed-content payload with `--algo lzma` at every LZMA-eligible
//! tier, run under headless DOSBox-X at the matching `cputype=`, and
//! assert byte-identical extraction.
//!
//! Parallel to `dosbox_aplib_large.rs`. With `LZMA_CHUNK_INPUT =
//! 16 KiB` (matches the aPLib chunk size), a 500 KiB payload spans
//! ~32 chunks per file, so the stub's per-chunk reset + decode loop
//! gets hammered end-to-end on a real-mode CPU emulation. Catches
//! the class of bug the single-chunk per-tier gate misses: an off-
//! by-one in xz_dec_microlzma_reset between chunks could pass the
//! small-fixture gate (each fixture is < 16 KiB, one chunk) but
//! corrupt the second chunk of any real payload.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't
//! blocked; runs in CI's `dosbox-x-integration` job via
//! `cargo test -- --ignored`.

use std::fs;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, wait_with_timeout, WaitError};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(600);
const PAYLOAD_SIZE: usize = 500 * 1024;

/// Same mixed-content payload `benchmark_tiers::synthesize_payload`
/// uses (kept inline for the same reason `dosbox_aplib_large.rs`
/// keeps it inline: this correctness gate shouldn't depend on the
/// benchmark harness).
fn synthesize_payload() -> Vec<u8> {
    let text = b"doskrunch phase 5 lzma payload\n";
    let mut out = Vec::with_capacity(PAYLOAD_SIZE);
    let mut lcg: u32 = 0xDECAFBAD;
    while out.len() < PAYLOAD_SIZE {
        for i in 0..256 {
            out.push(text[i % text.len()]);
            if out.len() >= PAYLOAD_SIZE {
                break;
            }
        }
        lcg = lcg.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let rand_run = ((lcg >> 16) & 0x3F) as usize;
        for _ in 0..rand_run {
            lcg = lcg.wrapping_mul(1_103_515_245).wrapping_add(12345);
            out.push((lcg >> 16) as u8);
            if out.len() >= PAYLOAD_SIZE {
                break;
            }
        }
    }
    out.truncate(PAYLOAD_SIZE);
    out
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_500kib_lzma_payload_across_lzma_tiers() {
    let payload = synthesize_payload();
    let bin = env!("CARGO_BIN_EXE_doskrunch");

    let tiers: &[(&str, &str)] = &[
        ("386", "386"),
        ("486", "486"),
        ("pentium", "pentium"),
        ("pentium-mmx", "pentium_mmx"),
        ("p2", "pentium_ii"),
        ("p3", "pentium_iii"),
    ];

    for (tier, cputype) in tiers {
        // Use a dedicated subdir per tier so the source PAYLOAD.BIN
        // doesn't get clobbered by an earlier tier's extraction.
        let rundir = tempfile::tempdir().expect("create tempdir");
        let rundir_path = rundir.path();
        fs::write(rundir_path.join("PAYLOAD.BIN"), &payload).expect("write source payload");

        let sfx_path = rundir_path.join("OUT.EXE");
        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .arg(rundir_path.join("PAYLOAD.BIN"))
            .args(["--algo", "lzma", "--target", tier])
            .status()
            .expect("spawn doskrunch pack");
        assert!(
            status.success(),
            "doskrunch pack failed for lzma tier {tier}: {status:?}"
        );

        // Remove the source so the SFX has to recreate it on extract.
        fs::remove_file(rundir_path.join("PAYLOAD.BIN")).expect("rm source payload");

        let conf_path = rundir_path.join("dosbox.conf");
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
                mount = rundir_path.display(),
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

        let extracted = locate_case_insensitive(rundir_path, "PAYLOAD.BIN")
            .unwrap_or_else(|| panic!("missing extracted PAYLOAD.BIN on lzma tier {tier}"));
        let body = fs::read(&extracted)
            .unwrap_or_else(|e| panic!("read extracted PAYLOAD.BIN (lzma tier {tier}): {e}"));
        assert_eq!(
            body.len(),
            payload.len(),
            "lzma tier {tier}: extracted PAYLOAD.BIN length mismatch ({} vs {})",
            body.len(),
            payload.len()
        );
        assert_eq!(
            body, payload,
            "lzma tier {tier}: extracted PAYLOAD.BIN body differs from source"
        );
    }
}
