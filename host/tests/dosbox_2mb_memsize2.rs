//! Phase 4 verify gate (PLAN.md §10): a 2 MiB payload SFX extracts
//! correctly under DOSBox-X with `memsize=2`, `xms=false`, `ems=false`,
//! `umb=false` — across all three shipped tiers.
//!
//! What this proves
//! ----------------
//! The chunked-decode path already shipped in Phase 2/3 (per-chunk read
//! → decompress into `g_buf` → write → repeat in `stubs/src/stub.c::main`,
//! with the host's `build_aplib_entry` splitting input at
//! `APLIB_CHUNK_INPUT = 16 KiB`) is supposed to keep the SFX's working
//! set bounded by the stub's small-model data segment (~35 KiB BSS:
//! `g_src` 18464 + `g_buf` 16384). The 500 KiB `dosbox_aplib_large.rs`
//! gate already exercises the per-chunk loop; this gate goes further by
//! pinning `memsize=2` (≈620 KiB free conventional after DOS overhead)
//! and explicitly disabling XMS/EMS/UMB so the stub can't lean on any
//! upper-memory backstop. A regression that accidentally turns the
//! depacker into "read whole payload, then decompress" would balloon
//! past `memsize=2` and panic with `out of memory` or seg-fault inside
//! DOSBox-X; byte-identical extraction at this `memsize` setting is the
//! end-to-end signal that the chunked path is doing its job.
//!
//! Payload shape
//! -------------
//! 2 MiB of mixed-content bytes (text + zeros + LCG-pseudo-random +
//! repeated patterns) — same distribution as
//! `benchmark_tiers::synthesize_payload`, scaled up. Compressible to
//! roughly 50% so the .EXE on disk stays around 1 MiB instead of
//! ballooning past 2 MiB; an incompressible random payload would make
//! the test mostly measure file-I/O wall-clock without exercising
//! the chunked decode loop any more than the 500 KiB gate already does.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, wait_with_timeout, WaitError};

/// 2 MiB exactly — matches PLAN.md §10 Phase 4 Verify wording.
const PAYLOAD_SIZE: usize = 2 * 1024 * 1024;

/// Generous wall-clock cap. At `memsize=2` and `cputype=8086` the
/// emulator chews through 2 MiB of decompression + INT 21h I/O at
/// IBM-PC speeds; on a modern host that's typically tens of seconds
/// but on a slow CI runner it can stretch toward 5 min. 10 min is the
/// belt-and-braces "definitely a hang" line.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(600);

/// Same byte distribution as `host/tests/benchmark_tiers.rs::synthesize_payload`,
/// inlined so this correctness gate doesn't import from the benchmark
/// harness. Mix per 1 KiB cycling block:
///   * 0–256:    rotating ASCII text
///   * 256–512:  zero run
///   * 512–768:  pseudo-random binary (deterministic LCG)
///   * 768–1024: repeated 16-byte pattern
fn synthesize_payload() -> Vec<u8> {
    let text = b"doskrunch phase 4 memsize=2 payload\n";
    let mut out = Vec::with_capacity(PAYLOAD_SIZE);
    let mut lcg: u32 = 0x9E37_79B1;
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
fn extracts_2mib_payload_under_memsize2_no_xms_ems() {
    let payload = synthesize_payload();
    assert_eq!(payload.len(), PAYLOAD_SIZE);

    let work_root = tempfile::tempdir().expect("create tempdir");
    let payload_path = work_root.path().join("payload.bin");
    fs::write(&payload_path, &payload).expect("write payload");

    let bin = env!("CARGO_BIN_EXE_doskrunch");

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

        // memsize=2 gives ~620 KiB conventional after DOS overhead.
        // xms/ems/umb=false stops DOSBox-X from quietly setting up
        // upper-memory backstops that would mask a stub regression.
        // PLAN.md §10 Phase 4 wording is "memsize=2 and no XMS/EMS";
        // umb=false is added for belt-and-braces — UMB allocation
        // can also inflate the resident DOS footprint.
        let conf_path = rundir_path.join("dosbox.conf");
        fs::write(
            &conf_path,
            format!(
                concat!(
                    "[cpu]\n",
                    "cputype={cputype}\n",
                    "core=normal\n",
                    "[dosbox]\n",
                    "memsize=2\n",
                    "[dos]\n",
                    "xms=false\n",
                    "ems=false\n",
                    "umb=false\n",
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
            "dosbox-x exited non-zero for tier {tier} at memsize=2: {dosbox_status:?}",
        );

        let extracted: PathBuf = locate_case_insensitive(rundir_path, "PAYLOAD.BIN")
            .unwrap_or_else(|| panic!("missing extracted payload for tier {tier}"));
        let body = fs::read(&extracted)
            .unwrap_or_else(|e| panic!("read extracted payload (tier {tier}): {e}"));
        assert_eq!(body.len(), payload.len(), "size mismatch tier {tier}");
        assert!(
            body == payload,
            "byte mismatch on tier {tier} at memsize=2"
        );
    }
}
