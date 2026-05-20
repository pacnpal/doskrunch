//! Visual (watch-it-yourself) DOSBox-X demo — NOT a CI gate.
//!
//! Unlike the headless `dosbox_*` correctness gates (which set
//! `SDL_VIDEODRIVER=dummy` and assert byte-identical extraction), this
//! one opens a REAL DOSBox-X window so you can watch the SFX extract on
//! screen. It packs the fixture set, runs the SFX, lists the result, and
//! `PAUSE`s so the window stays up until you press a key.
//!
//! Double-gated so it never runs unattended:
//!   * `#[ignore]` — skipped by a plain `cargo test`.
//!   * requires `DOSKRUNCH_VISUAL=1` — so even `cargo test -- --ignored`
//!     (and CI's headless sweep) skips it instead of trying to pop a
//!     window on a machine with no display. It is also intentionally not
//!     listed in any `dosbox-x-integration` shard in `.github/workflows`.
//!
//! Run it:
//!   DOSKRUNCH_VISUAL=1 cargo test --test dosbox_visual -- --ignored --nocapture
//!
//! Pick a tier/algo to watch (defaults: aplib / 8086):
//!   DOSKRUNCH_VISUAL=1 DOSKRUNCH_VISUAL_ALGO=lzma DOSKRUNCH_VISUAL_TARGET=pentium \
//!     cargo test --test dosbox_visual -- --ignored --nocapture

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{repo_root, wait_with_timeout, WaitError};

/// Generous cap: you're watching, and the run blocks on a `PAUSE`
/// waiting for your keypress. 10 minutes is plenty; if you wander off
/// the child is still cleaned up rather than stalling forever.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(600);

fn fixtures() -> &'static [&'static str] {
    &["hello.txt", "numbers.txt", "random.bin", "empty.bin"]
}

/// Map a `--target` tier to the matching DOSBox-X `cputype` spelling
/// (same table the correctness gates use).
fn cputype_for(target: &str) -> &'static str {
    match target {
        "8086" => "8086",
        "286" => "286",
        "386" => "386",
        "486" => "486",
        "pentium" => "pentium",
        "pentium-mmx" => "pentium_mmx",
        "p2" => "pentium_ii",
        "p3" => "pentium_iii",
        other => panic!("unknown --target {other}; pick 8086|286|386|486|pentium|pentium-mmx|p2|p3"),
    }
}

#[test]
#[ignore = "opens a DOSBox-X window; run with DOSKRUNCH_VISUAL=1 cargo test --test dosbox_visual -- --ignored --nocapture"]
fn watch_sfx_extract_in_dosbox() {
    if std::env::var_os("DOSKRUNCH_VISUAL").is_none() {
        eprintln!(
            "dosbox_visual: skipped (set DOSKRUNCH_VISUAL=1 to open a DOSBox-X window and watch)"
        );
        return;
    }

    let algo = std::env::var("DOSKRUNCH_VISUAL_ALGO").unwrap_or_else(|_| "aplib".to_string());
    let target = std::env::var("DOSKRUNCH_VISUAL_TARGET").unwrap_or_else(|_| "8086".to_string());
    let cputype = cputype_for(&target);

    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    // Pack the fixture set into the SFX we'll watch extract.
    let sfx_path = work_path.join("OUT.EXE");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();
    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .args(["--algo", &algo, "--target", &target])
        .args(&inputs)
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "doskrunch pack failed: {status:?}");
    let sfx_size = fs::metadata(&sfx_path).map(|m| m.len()).unwrap_or(0);
    eprintln!("packed {algo}/{target} SFX = {sfx_size} bytes; opening DOSBox-X window...");

    // dosbox.conf with a REAL display (no SDL_VIDEODRIVER=dummy, no
    // -nogui below). The autoexec runs the SFX, lists what landed, then
    // PAUSEs so the window stays up for you to read before it exits.
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
                "echo.\n",
                "echo === doskrunch SFX demo: {algo} / {target} ===\n",
                "echo.\n",
                "OUT.EXE\n",
                "echo.\n",
                "echo --- extracted into C: ---\n",
                "dir /w\n",
                "echo.\n",
                "pause\n",
                "exit\n",
            ),
            cputype = cputype,
            mount = work_path.display(),
            algo = algo,
            target = target,
        ),
    )
    .expect("write dosbox.conf");

    // No SDL_VIDEODRIVER=dummy and no -nogui: we want the emulation
    // window visible. `-exit` closes DOSBox-X when the autoexec `exit`
    // runs (after you press a key at the PAUSE).
    let mut dosbox = Command::new("dosbox-x")
        .arg("-conf")
        .arg(&conf_path)
        .arg("-exit")
        .spawn()
        .expect("spawn dosbox-x (is it installed?)");
    match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
        Ok(s) => assert!(s.success(), "dosbox-x exited non-zero: {s:?}"),
        Err(WaitError::Timeout) => {
            panic!("dosbox-x still open after {DOSBOX_TIMEOUT:?}; child was killed")
        }
        Err(WaitError::Wait(e)) => panic!("waiting on dosbox-x failed: {e}"),
    }
    eprintln!("dosbox_visual: window closed cleanly.");
}
