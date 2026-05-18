//! Phase 3 §10 timing harness: measure end-to-end SFX wall-clock for a
//! 500 KiB synthetic mixed-content payload at each shipped tier under
//! headless DOSBox-X and regenerate `tests/benchmarks/results.md`.
//!
//! NOT a correctness gate — this file is purely a measurement tool. The
//! large-payload multi-chunk correctness gate lives in
//! `host/tests/dosbox_aplib_large.rs` (runs in CI under `--ignored`,
//! asserts byte-identical extraction at every tier). PLAN.md §10's
//! 2–4× / 5–10× speedup ratios are tracked separately in
//! `tests/benchmarks/results.md` and `tasks/todo.md` — see the perf-gate
//! row in todo.md for the current "not met, awaiting user direction"
//! state. The harness here reports raw wall-clock; it does not assert
//! on the ratio so a measurement-substrate miss can't block the PR.
//!
//! Double-gated: `#[ignore]` AND `DOSKRUNCH_RUN_BENCHMARK=1`. CI's
//! `dosbox-x-integration` job runs `cargo test --workspace -- --ignored`
//! and would otherwise execute this test and silently rewrite the
//! committed `tests/benchmarks/results.md`; the env-var check gates
//! that off. Run locally with:
//!
//!     DOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy \
//!         cargo test --test benchmark_tiers -- --ignored --nocapture
//!
//! and commit the regenerated `tests/benchmarks/results.md` when the
//! numbers move.
//!
//! Caveats on the numbers (written into results.md too):
//!   * DOSBox-X CPU emulation cost varies with the host CPU and the
//!     guest cputype. Pentium is more expensive to emulate per
//!     instruction than 386, which partially offsets the 32-bit-
//!     register depacker's instruction-count win.
//!   * `cycles=auto` lets DOSBox-X dynamically tune throughput. The
//!     numbers below reflect total SFX wall-clock under that config,
//!     including DOS startup overhead (a few hundred ms regardless of
//!     payload).
//!   * The depacker is a small fraction of the SFX run. Most wall-clock
//!     time is DOS file I/O through INT 21h.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

/// Per-tier wall-clock cap. Even 4.77 MHz IBM-PC-class emulation should
/// chew through 500 KiB of aPLib in well under five minutes on any
/// modern host. Anything longer is a hang.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(300);

/// How many times to run each tier. Wall-clock is noisy at sub-second
/// granularity; the harness records min across N runs to filter out
/// scheduling jitter from the host.
const RUNS_PER_TIER: usize = 3;

/// Payload size — matches PLAN.md §10 Phase 3.
const PAYLOAD_SIZE: usize = 500 * 1024;

fn repo_root() -> PathBuf {
    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    host.parent().expect("host has a parent").to_path_buf()
}

/// Synthesize a deterministic mixed-content payload. The byte
/// distribution is intentionally non-random so that aPLib actually
/// compresses it (random data is incompressible by construction and
/// would make the benchmark mostly measure DOSBox's file-I/O loop).
///
/// Mix (per 1 KiB block, cycling):
///   * 0–256:    rotating ASCII text "doskrunch phase 3 benchmark payload\n"
///   * 256–512:  zero-run (compresses to a few aPLib match tokens)
///   * 512–768:  pseudo-random binary from a deterministic LCG
///   * 768–1024: repeated 16-byte pattern (AAAA…BBBB…CCCC…)
fn synthesize_payload() -> Vec<u8> {
    let text = b"doskrunch phase 3 benchmark payload\n";
    let mut out = Vec::with_capacity(PAYLOAD_SIZE);
    let mut lcg: u32 = 0xDECAFBAD;
    while out.len() < PAYLOAD_SIZE {
        // 256 bytes of ASCII text
        for i in 0..256 {
            out.push(text[i % text.len()]);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        // 256 bytes of zeros
        for _ in 0..256 {
            out.push(0);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        // 256 bytes of pseudo-random
        for _ in 0..256 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            out.push((lcg >> 16) as u8);
            if out.len() == PAYLOAD_SIZE {
                return out;
            }
        }
        // 256 bytes of 16-byte pattern runs
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

struct TierResult {
    tier: &'static str,
    cputype: &'static str,
    sfx_size: u64,
    wall_clock_min_ms: u128,
    runs: Vec<u128>,
}

#[test]
#[ignore = "needs dosbox-x AND DOSKRUNCH_RUN_BENCHMARK=1; regenerates tests/benchmarks/results.md"]
fn benchmark_tier_decompression() {
    // Double-gate so CI's `cargo test --workspace -- --ignored` run
    // (.github/workflows/test.yml :: dosbox-x-integration) doesn't
    // silently rewrite the committed results.md file. Local devs opt
    // in by setting the env var; everyone else gets a fast skip.
    if std::env::var_os("DOSKRUNCH_RUN_BENCHMARK").is_none() {
        eprintln!(
            "benchmark_tier_decompression: skipped (set DOSKRUNCH_RUN_BENCHMARK=1 to run)"
        );
        return;
    }

    let root = repo_root();
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();

    // One payload file for all three tiers — the same input bytes get
    // packed by each --target to keep the per-tier compressed size
    // comparable. (The bytes are the same; the embedded stub differs.)
    let payload = synthesize_payload();
    assert_eq!(payload.len(), PAYLOAD_SIZE);
    let payload_path = root.join("target").join("bench_payload.bin");
    fs::create_dir_all(payload_path.parent().unwrap()).expect("mkdir target");
    fs::write(&payload_path, &payload).expect("write payload");

    let bin = env!("CARGO_BIN_EXE_doskrunch");
    let tiers: &[(&str, &str)] = &[
        ("8086", "8086"),
        ("386", "386"),
        ("pentium", "pentium"),
    ];

    let mut results: Vec<TierResult> = Vec::new();
    for (tier, cputype) in tiers {
        let sfx_path = work_path.join(format!("OUT_{tier}.EXE"));
        let status = Command::new(bin)
            .arg("pack")
            .arg(&sfx_path)
            .arg(&payload_path)
            .args(["--algo", "aplib", "--target", tier])
            .status()
            .expect("spawn doskrunch pack");
        assert!(status.success(), "pack failed for tier {tier}");

        let sfx_size = fs::metadata(&sfx_path).expect("stat sfx").len();

        // dosbox-x needs the SFX named on an 8.3 path inside the mount.
        // We rename per-run so each tier's run-dir is independent.
        let mut runs_ms: Vec<u128> = Vec::with_capacity(RUNS_PER_TIER);
        for run in 0..RUNS_PER_TIER {
            let rundir = tempfile::tempdir().expect("rundir");
            let rundir_path = rundir.path();
            let runsfx = rundir_path.join("OUT.EXE");
            fs::copy(&sfx_path, &runsfx).expect("copy sfx into rundir");

            let conf_path = rundir_path.join("dosbox.conf");
            fs::write(
                &conf_path,
                format!(
                    concat!(
                        "[cpu]\n",
                        "cputype={cputype}\n",
                        "core=normal\n",
                        "cycles=auto\n",
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

            let started = Instant::now();
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
                    "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} (tier {tier} run {run}); child was killed"
                ),
                Err(WaitError::Wait(e)) => panic!(
                    "waiting on dosbox-x failed: {e} (tier {tier} run {run}); child was killed"
                ),
            };
            assert!(
                dosbox_status.success(),
                "dosbox-x exited non-zero for tier {tier} run {run}: {dosbox_status:?}"
            );
            let elapsed = started.elapsed().as_millis();
            runs_ms.push(elapsed);

            // Sanity check: the extracted payload must match the input
            // exactly. We verify on every run so a flaky depacker can't
            // hide behind aggregate timing.
            let extracted = locate_case_insensitive(rundir_path, "BENCH_PAYLOAD.BIN")
                .or_else(|| locate_case_insensitive(rundir_path, "BENCH_PA.BIN"))
                .expect("locate extracted payload");
            let body = fs::read(&extracted).expect("read extracted");
            assert_eq!(body.len(), payload.len(), "size mismatch tier {tier}");
            assert!(body == payload, "byte mismatch tier {tier}");
        }
        let wall_clock_min_ms = *runs_ms.iter().min().unwrap();
        results.push(TierResult {
            tier,
            cputype,
            sfx_size,
            wall_clock_min_ms,
            runs: runs_ms,
        });
    }

    write_results_markdown(&root, &results, &payload);

    // Print the ratios for inspection. This is a measurement harness,
    // not a gate — the byte-identical correctness check lives in
    // dosbox_aplib_large.rs (and runs in CI under --ignored without
    // requiring the env var). The PLAN.md §10 2–4× / 5–10× speedup
    // expectation is tracked in tasks/todo.md as an unmet perf gate;
    // this harness doesn't assert it because end-to-end SFX wall-clock
    // under DOSBox-X cycles=auto isn't a reliable signal for relative
    // depacker performance (per-cputype emulation cost variance + DOS
    // startup + INT 21h I/O are all in the path). An assertion here
    // would either spuriously fail on measurement noise or pass on a
    // genuinely slow port, neither of which is useful. Closing the
    // perf gate needs isolated depacker timing (stub-side INT 1Ah
    // cycle counter) or real-iron data; see tasks/todo.md.
    let baseline = results[0].wall_clock_min_ms.max(1);
    for r in &results {
        let ratio = baseline as f64 / r.wall_clock_min_ms.max(1) as f64;
        println!(
            "tier={tier:8} wall={ms:6} ms  ratio_vs_8086={ratio:.2}x",
            tier = r.tier,
            ms = r.wall_clock_min_ms,
            ratio = ratio,
        );
    }
}

fn write_results_markdown(root: &Path, results: &[TierResult], payload: &[u8]) {
    let dest = root.join("tests").join("benchmarks").join("results.md");
    fs::create_dir_all(dest.parent().unwrap()).expect("mkdir benchmarks");

    let baseline = results
        .iter()
        .find(|r| r.tier == "8086")
        .map(|r| r.wall_clock_min_ms.max(1))
        .unwrap_or(1);

    let mut md = String::new();
    md.push_str("# Tier decompression benchmark\n\n");
    md.push_str(&format!(
        "Synthetic mixed-content payload: {} KiB ({} bytes) — text + zeros + LCG-random + repeated patterns. \
        See `host/tests/benchmark_tiers.rs::synthesize_payload` for the exact distribution.\n\n",
        payload.len() / 1024,
        payload.len(),
    ));
    md.push_str("Measurement: end-to-end SFX wall-clock under headless DOSBox-X with `cycles=auto`, \
        min across 3 runs per tier. The benchmark is `#[ignore]`-gated AND env-var-gated \
        (`DOSKRUNCH_RUN_BENCHMARK=1`) so CI's `--ignored` run doesn't silently rewrite this \
        file. Run locally with:\n\n");
    md.push_str("```bash\nDOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy cargo test --test benchmark_tiers -- --ignored --nocapture\n```\n\n");
    md.push_str("| Tier | cputype | SFX size (bytes) | Wall clock min (ms) | Ratio vs 8086 |\n");
    md.push_str("|------|---------|------------------|----------------------|----------------|\n");
    for r in results {
        let ratio = baseline as f64 / r.wall_clock_min_ms.max(1) as f64;
        md.push_str(&format!(
            "| {tier} | {cputype} | {sfx} | {ms} | {ratio:.2}× |\n",
            tier = r.tier,
            cputype = r.cputype,
            sfx = r.sfx_size,
            ms = r.wall_clock_min_ms,
            ratio = ratio,
        ));
    }
    md.push_str("\n## Per-run detail\n\n");
    md.push_str("| Tier | Run 1 (ms) | Run 2 (ms) | Run 3 (ms) |\n");
    md.push_str("|------|------------|------------|------------|\n");
    for r in results {
        let cells: Vec<String> = (0..RUNS_PER_TIER)
            .map(|i| r.runs.get(i).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        md.push_str(&format!(
            "| {tier} | {} |\n",
            cells.join(" | "),
            tier = r.tier,
        ));
    }
    md.push_str(
        "\n## Caveats\n\n\
         * `cycles=auto` is DOSBox-X's heuristic throughput scaler; results vary with host \
           CPU and DOSBox-X version. Pentium emulation is more expensive per guest instruction \
           than 386 emulation, which partially offsets the speed-optimized depacker's win.\n\
         * Most wall-clock time is DOS startup overhead and INT 21h file I/O, not the depacker. \
           The depacker is a small slice of the total run.\n\
         * PLAN.md §10 Phase 3 Verify explicitly lists \"386 is 2-4x faster than 8086, pentium \
           is 5-10x faster\" as the expected benchmark outcome — it does not qualify that as \
           real-hardware-only. The numbers above miss that gate. What we can assert from this \
           harness is correctness, not relative depacker performance: the DOSBox-X correctness \
           gates (`dosbox_8086`, `dosbox_aplib_8086`, `dosbox_aplib_386`, \
           `dosbox_aplib_pentium`, and the multi-chunk `dosbox_aplib_large`) all extract \
           byte-identical payloads at every tier. Where the speedup went is currently a \
           hypothesis, not a measurement: DOSBox-X with `cycles=auto` is a noisy substrate \
           for relative-CPU comparison (per-cputype emulation cost varies with the host \
           CPU and DOSBox-X build), and DOS startup + INT 21h file I/O likely dominate the \
           2-second wall-clock. Confirming or refuting the depacker-is-fine hypothesis needs \
           either isolated depacker timing (a stub-side INT 1Ah cycle counter that times \
           just the depacker call, not the whole SFX) or real-iron measurement (86Box or a \
           real 386 / Pentium box). Phase 3 ships the ports and the correctness gates; the \
           perf gate is acknowledged unmet here for the user to direct.\n\
         * The benchmark is double-gated (`#[ignore]` + `DOSKRUNCH_RUN_BENCHMARK=1`); CI's \
           `cargo test --workspace -- --ignored` run skips it via the env-var check, so this \
           file is only regenerated by a local opt-in run. Commit a refreshed copy when the \
           numbers move.\n",
    );
    fs::write(&dest, md).expect("write results.md");
    eprintln!("wrote {}", dest.display());
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
