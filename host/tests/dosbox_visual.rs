//! Visual (watch-it-yourself) DOSBox-X demo — NOT a CI gate.
//!
//! Unlike the headless `dosbox_*` correctness gates (which set
//! `SDL_VIDEODRIVER=dummy` and assert byte-identical extraction), this
//! one opens REAL DOSBox-X windows so you can watch the SFX extract on
//! screen, with a little DOSKrunch banner + version. By default it walks
//! every CPU tier valid for the chosen algo, one window per tier; press a
//! key at the DOS `PAUSE` in each window to advance to the next.
//!
//! Double-gated so it never runs unattended:
//!   * `#[ignore]` — skipped by a plain `cargo test`.
//!   * requires `DOSKRUNCH_VISUAL=1` — so even `cargo test -- --ignored`
//!     (and CI's headless sweep) skips it instead of trying to pop a
//!     window on a machine with no display. It is also intentionally not
//!     listed in any `dosbox-x-integration` shard in `.github/workflows`.
//!
//! Run it (walks all tiers for aplib by default):
//!   DOSKRUNCH_VISUAL=1 cargo test --test dosbox_visual -- --ignored --nocapture
//!
//! Pick an algo (walks the tiers valid for it; lzma is 386+):
//!   DOSKRUNCH_VISUAL=1 DOSKRUNCH_VISUAL_ALGO=lzsa2 \
//!     cargo test --test dosbox_visual -- --ignored --nocapture
//!
//! Or pin a single tier:
//!   DOSKRUNCH_VISUAL=1 DOSKRUNCH_VISUAL_ALGO=lzma DOSKRUNCH_VISUAL_TARGET=pentium \
//!     cargo test --test dosbox_visual -- --ignored --nocapture

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{cputype_for, fixtures, repo_root, wait_with_timeout, WaitError};

/// Generous per-window cap: you're watching, and each run blocks on a
/// `PAUSE` waiting for your keypress. 10 minutes is plenty; if you wander
/// off the child is still cleaned up rather than stalling forever.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(600);

/// Every shipped CPU tier, broadest to tightest.
const ALL_TIERS: &[&str] = &[
    "8086",
    "286",
    "386",
    "486",
    "pentium",
    "pentium-mmx",
    "p2",
    "p3",
];

/// DOSKrunch version for the banner: prefer the most recent tag reachable
/// from HEAD (what `git describe --tags --abbrev=0` reports — releases are
/// tagged, so in practice this is the release version), falling back to the
/// crate version when the tree has no tags yet.
fn doskrunch_version() -> String {
    let raw = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .current_dir(repo_root())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("v{} (untagged)", env!("CARGO_PKG_VERSION")));
    // This is interpolated into DOS `echo` lines, and a tag name is
    // otherwise free-form. Strip anything that isn't a safe printable so
    // a weird tag can't inject DOS metacharacters (| < > & ^ %) into the
    // generated dosbox.conf.
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " ._()+-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Is `algo` valid on `tier`? Only LZMA is restricted (386+).
fn algo_supports_tier(algo: &str, tier: &str) -> bool {
    algo != "lzma" || !(tier == "8086" || tier == "286")
}

#[test]
#[ignore = "opens DOSBox-X windows; run with DOSKRUNCH_VISUAL=1 cargo test --test dosbox_visual -- --ignored --nocapture"]
fn watch_sfx_extract_in_dosbox() {
    // Require the exact value "1" (not mere presence): this test opens
    // real windows, so a stray `DOSKRUNCH_VISUAL=0` must NOT trigger it.
    if std::env::var("DOSKRUNCH_VISUAL").as_deref() != Ok("1") {
        eprintln!(
            "dosbox_visual: skipped (set DOSKRUNCH_VISUAL=1 to open DOSBox-X windows and watch)"
        );
        return;
    }

    let algo = std::env::var("DOSKRUNCH_VISUAL_ALGO").unwrap_or_else(|_| "aplib".to_string());
    // Allowlist the algo: it's interpolated into the dosbox.conf banner, and
    // this guarantees it's one of the known-safe literals (no DOS
    // metacharacters can reach the config). `tier` is likewise validated by
    // `cputype_for`, and `version` is sanitized in `doskrunch_version`.
    const ALGOS: &[&str] = &["aplib", "stored", "lzma", "lzsa2"];
    assert!(
        ALGOS.contains(&algo.as_str()),
        "unknown DOSKRUNCH_VISUAL_ALGO {algo:?}; expected one of {ALGOS:?}"
    );
    let version = doskrunch_version();

    // Which tiers to walk: a single one if DOSKRUNCH_VISUAL_TARGET is set,
    // otherwise every tier the algo supports.
    let candidates: Vec<String> = match std::env::var("DOSKRUNCH_VISUAL_TARGET") {
        Ok(t) => vec![t],
        Err(_) => ALL_TIERS.iter().map(|s| s.to_string()).collect(),
    };
    let tiers: Vec<String> = candidates
        .into_iter()
        .filter(|t| {
            let ok = algo_supports_tier(&algo, t);
            if !ok {
                eprintln!("  (skipping {algo}/{t}: LZMA requires --target 386 or higher)");
            }
            ok
        })
        .collect();
    assert!(
        !tiers.is_empty(),
        "no tier valid for algo {algo} (LZMA needs 386+)"
    );

    let root = repo_root();
    let fixtures_dir = root.join("tests").join("fixtures");
    let inputs: Vec<PathBuf> = fixtures().iter().map(|f| fixtures_dir.join(f)).collect();
    let bin = env!("CARGO_BIN_EXE_doskrunch");

    let total = tiers.len();
    eprintln!("DOSKrunch {version}: watching {total} tier(s) with algo {algo} — press a key in each DOS window to advance");

    for (i, tier) in tiers.iter().enumerate() {
        let idx = i + 1;
        let cputype = cputype_for(tier);

        // Fresh tempdir per tier so each window has its own clean C:.
        let work = tempfile::tempdir().expect("create tempdir");
        let work_path = work.path();
        let sfx_path = work_path.join("OUT.EXE");

        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .args(["--algo", &algo, "--target", tier])
            .args(&inputs)
            .status()
            .expect("spawn doskrunch pack");
        assert!(status.success(), "doskrunch pack failed for {algo}/{tier}: {status:?}");
        let sfx_size = fs::metadata(&sfx_path).expect("stat packed SFX").len();
        eprintln!("[{idx}/{total}] {algo}/{tier}: {sfx_size} bytes — opening DOSBox-X window...");

        // dosbox.conf with a REAL display (no SDL_VIDEODRIVER=dummy, no
        // -nogui). The autoexec prints a DOSKrunch banner, runs the SFX,
        // lists what landed, then PAUSEs so the window stays up to read.
        // The banner art uses only DOS-echo-safe characters (no | < > & %).
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
                    "cls\n",
                    "echo.\n",
                    "echo   ===============================================\n",
                    "echo      [#####]   D O S K r u n c h   {version}\n",
                    "echo      squeeze it down, run it on real DOS\n",
                    "echo   ===============================================\n",
                    "echo      tier {idx} of {total}:  {tier}        algo:  {algo}\n",
                    "echo.\n",
                    "echo   unpacking...\n",
                    "OUT.EXE\n",
                    "echo.\n",
                    "echo   --- crunched files now on C: ---\n",
                    "dir /w\n",
                    "echo.\n",
                    "pause\n",
                    "exit\n",
                ),
                cputype = cputype,
                mount = work_path.display(),
                version = version,
                idx = idx,
                total = total,
                tier = tier,
                algo = algo,
            ),
        )
        .expect("write dosbox.conf");

        let mut dosbox = Command::new("dosbox-x")
            .arg("-conf")
            .arg(&conf_path)
            .arg("-exit")
            .spawn()
            .expect("spawn dosbox-x (is it installed?)");
        match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
            Ok(s) => assert!(s.success(), "dosbox-x exited non-zero ({algo}/{tier}): {s:?}"),
            Err(WaitError::Timeout) => {
                panic!("dosbox-x still open after {DOSBOX_TIMEOUT:?} ({algo}/{tier}); child was killed")
            }
            Err(WaitError::Wait(e)) => panic!("waiting on dosbox-x failed: {e} ({algo}/{tier})"),
        }
    }
    eprintln!("dosbox_visual: all {total} window(s) closed cleanly.");
}
