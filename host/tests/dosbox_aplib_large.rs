//! Phase 3 multi-chunk correctness gate: pack a 500 KiB synthetic
//! mixed-content payload with `--algo aplib` at each of the three
//! shipped tiers, run the SFX under headless DOSBox-X at the matching
//! `cputype=`, and assert byte-identical extraction.
//!
//! The small-fixture DOSBox-X tests (`dosbox_aplib_{8086,386,pentium}.rs`)
//! exercise the depacker on payloads that each fit inside a single
//! 16 KiB aPLib chunk. This file fills the gap: at 500 KiB the payload
//! is split across ~32 chunks per file, so the stub's chunk loop
//! (read header → decompress into g_buf → write → repeat) and the host's
//! `build_aplib_entry` chunk-cap logic both get exercised end-to-end on
//! a real-mode CPU emulation.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.
//! Does NOT write any tracked files — the timing/measurement harness in
//! `benchmark_tiers.rs` is opt-in via `DOSKRUNCH_RUN_BENCHMARK=1` and
//! handles the optional `tests/benchmarks/results.md` regeneration.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(300);
const PAYLOAD_SIZE: usize = 500 * 1024;

fn repo_root() -> PathBuf {
    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    host.parent().expect("host has a parent").to_path_buf()
}

/// Deterministic mixed-content payload — same distribution as
/// `benchmark_tiers::synthesize_payload`. Kept inline here so this
/// correctness gate doesn't depend on the benchmark harness; per the
/// Phase 3 brief, test-utility consolidation lands in Phase 4.
fn synthesize_payload() -> Vec<u8> {
    let text = b"doskrunch phase 3 benchmark payload\n";
    let mut out = Vec::with_capacity(PAYLOAD_SIZE);
    let mut lcg: u32 = 0xDECAFBAD;
    while out.len() < PAYLOAD_SIZE {
        for i in 0..256 {
            out.push(text[i % text.len()]);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        for _ in 0..256 {
            out.push(0);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        for _ in 0..256 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            out.push((lcg >> 16) as u8);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        let pat_idx = (out.len() / 256) as u8;
        for _ in 0..16 {
            for _ in 0..16 {
                out.push(b'A'.wrapping_add(pat_idx % 26));
                if out.len() == PAYLOAD_SIZE {
                    return out;
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_500kib_multichunk_payload_across_tiers() {
    let root = repo_root();
    let payload = synthesize_payload();
    assert_eq!(payload.len(), PAYLOAD_SIZE);

    let work_root = tempfile::tempdir().expect("create tempdir");
    let payload_path = work_root.path().join("payload.bin");
    fs::write(&payload_path, &payload).expect("write payload");

    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let _ = root;

    for (tier, cputype) in &[("8086", "8086"), ("386", "386"), ("pentium", "pentium")] {
        let rundir = tempfile::tempdir().expect("rundir");
        let rundir_path = rundir.path();
        let sfx_path = rundir_path.join("OUT.EXE");

        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .arg(&payload_path)
            .args(["--algo", "aplib", "--target", tier])
            .status()
            .expect("spawn doskrunch pack");
        assert!(status.success(), "doskrunch pack failed for tier {tier}: {status:?}");

        let conf_path = rundir_path.join("dosbox.conf");
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
                "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} (tier {tier}); child was killed"
            ),
            Err(WaitError::Wait(e)) => panic!(
                "waiting on dosbox-x failed: {e} (tier {tier}); child was killed"
            ),
        };
        assert!(
            dosbox_status.success(),
            "dosbox-x exited non-zero for tier {tier}: {dosbox_status:?}",
        );

        // Source name `payload.bin` is already valid 8.3 ASCII, but the
        // host's name83 mangler uppercases on the wire, and DOSBox-X
        // writes uppercase 8.3 names on the host. Case-insensitive
        // lookup matches that behaviour.
        let extracted = locate_case_insensitive(rundir_path, "PAYLOAD.BIN")
            .unwrap_or_else(|| panic!("missing extracted payload for tier {tier}"));
        let body = fs::read(&extracted)
            .unwrap_or_else(|e| panic!("read extracted payload (tier {tier}): {e}"));
        assert_eq!(body.len(), payload.len(), "size mismatch tier {tier}");
        assert!(
            body == payload,
            "byte mismatch on tier {tier} multi-chunk extraction"
        );
    }
}

enum WaitError {
    Timeout,
    Wait(std::io::Error),
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<ExitStatus, WaitError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Ok(s),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WaitError::Timeout);
                }
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WaitError::Wait(e));
            }
        }
    }
}

fn locate_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(entry.path());
        }
    }
    None
}
