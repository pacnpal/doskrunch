//! Phase 4 stored-mode upper-bound DOSBox-X gate.
//!
//! `--chunk-size` for `--algo stored` now accepts values up to
//! `u16::MAX = 65535`, which is larger than the stub's `BUF_SIZE = 16384`
//! copy-buffer. The stub's stored branch calls `copy_bytes(self, out, csize)`,
//! which loops `BUF_SIZE`-sized reads until the whole chunk is consumed —
//! so a 65535-byte chunk takes 4 iterations (16384 + 16384 + 16384 + 16383).
//! Before this gate, that multi-iteration `copy_bytes` path had no
//! end-to-end coverage:
//!
//!   * `dosbox_stored_all_tiers.rs` uses the small fixture set; every
//!     chunk fits in a single `copy_bytes` iteration.
//!   * `dosbox_aplib_large.rs` and `dosbox_2mb_memsize2.rs` exercise the
//!     aplib branch, which goes through `g_src` / `aplib_depack` / `g_buf`
//!     and a single write — not through `copy_bytes`.
//!
//! This gate packs a 200 KiB (204 800 B) payload with
//! `--algo stored --chunk-size 65535` at `--target 8086` and runs it
//! under DOSBox-X with `cputype=8086`. The chunk layout is
//! 204_800 / 65535 = 4 chunks (3 × 65535 + 1 × 8195), and each of the
//! three 65535-byte chunks forces multiple `copy_bytes` iterations.
//! Byte-identical extraction proves the loop's chunk-handoff is correct.
//!
//! Source-file layout mirrors `dosbox_timestamps.rs`: the source
//! `payload.bin` lives in a separate `srcdir` outside the DOSBox-X
//! mount, so the case-insensitive lookup `locate_case_insensitive(
//! rundir, "PAYLOAD.BIN")` can't accidentally match the original
//! source on a case-insensitive filesystem and pass without
//! verifying what the stub wrote.
//!
//! `#[ignore]`-gated so contributors without `dosbox-x` aren't blocked;
//! runs in CI's `dosbox-x-integration` job via `cargo test -- --ignored`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, wait_with_timeout, WaitError};

/// 200 KiB — enough to force multiple 65535-byte chunks and within
/// the host's `unpack` 256 MiB per-file cap. The payload is a simple
/// counter pattern so we can verify byte-identical extraction without
/// depending on the benchmark synthesizer.
const PAYLOAD_SIZE: usize = 200 * 1024;

const DOSBOX_TIMEOUT: Duration = Duration::from_secs(180);

#[test]
#[ignore = "needs dosbox-x installed; run with `cargo test -- --ignored`"]
fn extracts_stored_payload_with_max_chunk_size_under_8086() {
    let payload: Vec<u8> = (0..PAYLOAD_SIZE).map(|i| (i & 0xff) as u8).collect();

    let srcdir = tempfile::tempdir().expect("create srcdir");
    let rundir = tempfile::tempdir().expect("create rundir");
    let work_path = rundir.path();

    // Source OUTSIDE the DOSBox-X mount so the case-insensitive
    // lookup below can't match the original `payload.bin` instead of
    // the extracted `PAYLOAD.BIN` on a case-insensitive filesystem.
    let src = srcdir.path().join("payload.bin");
    fs::write(&src, &payload).expect("write source payload");

    let sfx_path = work_path.join("OUT.EXE");
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(&src)
        .args([
            "--algo",
            "stored",
            "--target",
            "8086",
            "--chunk-size",
            "65535",
        ])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed: {status:?}");

    let conf_path = work_path.join("dosbox.conf");
    fs::write(
        &conf_path,
        format!(
            concat!(
                "[cpu]\n",
                "cputype=8086\n",
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
    let status = match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
        Ok(s) => s,
        Err(WaitError::Timeout) => {
            panic!("dosbox-x did not exit within {DOSBOX_TIMEOUT:?}; child was killed")
        }
        Err(WaitError::Wait(e)) => panic!("waiting on dosbox-x failed: {e}; child was killed"),
    };
    assert!(
        status.success(),
        "dosbox-x exited non-zero on stored max-chunk extraction: {status:?}",
    );

    let extracted: PathBuf =
        locate_case_insensitive(work_path, "PAYLOAD.BIN").expect("missing PAYLOAD.BIN");
    let body = fs::read(&extracted).expect("read extracted");
    assert_eq!(body.len(), payload.len(), "size mismatch");
    assert!(
        body == payload,
        "byte mismatch on stored extraction with 65535-byte chunks — \
         the stub's copy_bytes multi-iteration path may be broken"
    );
}
