//! Phase 3/5 timing harness: measure decode-only time for aPLib vs LZMA
//! on shared 386+ tiers under headless DOSBox-X and regenerate
//! `tests/benchmarks/results.md`.
//!
//! NOT a correctness gate — this file is purely a measurement tool. The
//! large-payload multi-chunk correctness gates live in
//! `host/tests/dosbox_aplib_large.rs` and `host/tests/dosbox_lzma_large.rs`.
//! This harness reports decode-only BIOS timer ticks collected from the
//! guest stubs.
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
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

mod common;
use common::{locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

/// Per-tier wall-clock cap. Even 4.77 MHz IBM-PC-class emulation should
/// chew through 500 KiB of aPLib in well under five minutes on any
/// modern host. Anything longer is a hang.
const DOSBOX_TIMEOUT: Duration = Duration::from_secs(300);

/// How many times to run each (algorithm, tier) pair.
const RUNS_PER_TIER: usize = 3;

/// Payload size — matches PLAN.md §10 Phase 3.
const PAYLOAD_SIZE: usize = 500 * 1024;

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
        let zeros_take = (PAYLOAD_SIZE - out.len()).min(256);
        out.resize(out.len() + zeros_take, 0);
        if out.len() == PAYLOAD_SIZE {
            return out;
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
    aplib_sfx_size: u64,
    lzma_sfx_size: u64,
    aplib_ticks_min: u32,
    lzma_ticks_min: u32,
    aplib_runs_ticks: Vec<u32>,
    lzma_runs_ticks: Vec<u32>,
}

#[test]
#[ignore = "needs dosbox-x AND DOSKRUNCH_RUN_BENCHMARK=1; regenerates tests/benchmarks/results.md"]
fn benchmark_tier_decompression() {
    // Double-gate so CI's `cargo test --workspace -- --ignored` run
    // (.github/workflows/test.yml :: dosbox-x-integration) doesn't
    // silently rewrite the committed results.md file. Local devs opt
    // in by setting the env var; everyone else gets a fast skip.
    if std::env::var_os("DOSKRUNCH_RUN_BENCHMARK").is_none() {
        eprintln!("benchmark_tier_decompression: skipped (set DOSKRUNCH_RUN_BENCHMARK=1 to run)");
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
    let tiers: &[(&str, &str)] = &[("386", "386"), ("486", "486"), ("pentium", "pentium")];

    let mut results: Vec<TierResult> = Vec::new();
    for (tier, cputype) in tiers {
        let aplib_sfx_path = work_path.join(format!("OUT_APLIB_{tier}.EXE"));
        let lzma_sfx_path = work_path.join(format!("OUT_LZMA_{tier}.EXE"));

        let aplib_pack = Command::new(bin)
            .arg("pack")
            .arg(&aplib_sfx_path)
            .arg(&payload_path)
            .args(["--algo", "aplib", "--target", tier])
            .status()
            .expect("spawn doskrunch pack aplib");
        assert!(aplib_pack.success(), "aplib pack failed for tier {tier}");

        let lzma_pack = Command::new(bin)
            .arg("pack")
            .arg(&lzma_sfx_path)
            .arg(&payload_path)
            .args(["--algo", "lzma", "--target", tier])
            .status()
            .expect("spawn doskrunch pack lzma");
        assert!(lzma_pack.success(), "lzma pack failed for tier {tier}");

        let aplib_sfx_size = fs::metadata(&aplib_sfx_path).expect("stat aplib sfx").len();
        let lzma_sfx_size = fs::metadata(&lzma_sfx_path).expect("stat lzma sfx").len();

        let aplib_runs_ticks = run_algo_timing(&aplib_sfx_path, &payload, tier, cputype, "aplib");
        let lzma_runs_ticks = run_algo_timing(&lzma_sfx_path, &payload, tier, cputype, "lzma");

        results.push(TierResult {
            tier,
            cputype,
            aplib_sfx_size,
            lzma_sfx_size,
            aplib_ticks_min: *aplib_runs_ticks.iter().min().unwrap(),
            lzma_ticks_min: *lzma_runs_ticks.iter().min().unwrap(),
            aplib_runs_ticks,
            lzma_runs_ticks,
        });
    }

    write_results_markdown(&root, &results, &payload);

    for r in &results {
        let ratio = r.lzma_ticks_min as f64 / r.aplib_ticks_min.max(1) as f64;
        println!(
            "tier={tier:8} aplib_ticks={aplib:6} lzma_ticks={lzma:6} lzma/aplib={ratio:.2}x",
            tier = r.tier,
            aplib = r.aplib_ticks_min,
            lzma = r.lzma_ticks_min,
            ratio = ratio,
        );
    }
}

fn parse_decode_ticks(path: &Path) -> u32 {
    let text = fs::read_to_string(path).expect("read DKTIME.TXT");
    let value = text
        .trim()
        .strip_prefix("DECODE_TICKS=")
        .expect("timing file format");
    u32::from_str_radix(value, 16).expect("decode ticks hex")
}

fn run_algo_timing(
    sfx_path: &Path,
    payload: &[u8],
    tier: &str,
    cputype: &str,
    algo: &str,
) -> Vec<u32> {
    let mut runs_ticks: Vec<u32> = Vec::with_capacity(RUNS_PER_TIER);
    for run in 0..RUNS_PER_TIER {
        let rundir = tempfile::tempdir().expect("rundir");
        let rundir_path = rundir.path();
        let runsfx = rundir_path.join("OUT.EXE");
        fs::copy(sfx_path, &runsfx).expect("copy sfx into rundir");

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
                    "set DOSKRUNCH_BENCH_TIMING=1\n",
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
                "dosbox-x did not exit within {DOSBOX_TIMEOUT:?} (algo {algo} tier {tier} run {run}); child was killed"
            ),
            Err(WaitError::Wait(e)) => panic!(
                "waiting on dosbox-x failed: {e} (algo {algo} tier {tier} run {run}); child was killed"
            ),
        };
        assert!(
            dosbox_status.success(),
            "dosbox-x exited non-zero for algo {algo} tier {tier} run {run}: {dosbox_status:?}"
        );

        let extracted = locate_case_insensitive(rundir_path, "BENCH_PAYLOAD.BIN")
            .or_else(|| locate_case_insensitive(rundir_path, "BENCH_PA.BIN"))
            .expect("locate extracted payload");
        let body = fs::read(&extracted).expect("read extracted");
        assert_eq!(
            body.len(),
            payload.len(),
            "size mismatch algo {algo} tier {tier}"
        );
        assert!(
            body == payload,
            "byte mismatch algo {algo} tier {tier} run {run}"
        );

        let timing_path =
            locate_case_insensitive(rundir_path, "DKTIME.TXT").expect("locate DKTIME.TXT");
        runs_ticks.push(parse_decode_ticks(&timing_path));
    }
    runs_ticks
}

fn ticks_to_ms(ticks: u32) -> f64 {
    (ticks as f64) * (1000.0 / 18.20648)
}

fn write_results_markdown(root: &Path, results: &[TierResult], payload: &[u8]) {
    let dest = root.join("tests").join("benchmarks").join("results.md");
    fs::create_dir_all(dest.parent().unwrap()).expect("mkdir benchmarks");

    let mut md = String::new();
    md.push_str("# Decode-only benchmark (aPLib vs LZMA)\n\n");
    md.push_str(&format!(
        "Synthetic mixed-content payload: {} KiB ({} bytes) — text + zeros + LCG-random + repeated patterns. \
        See `host/tests/benchmark_tiers.rs::synthesize_payload` for the exact distribution.\n\n",
        payload.len() / 1024,
        payload.len(),
    ));
    md.push_str("Measurement: decode-only BIOS timer ticks (`INT 1Ah, AH=00h`) gathered inside the guest stubs around the depacker call (`aplib_depack` for aPLib and `xz_dec_microlzma_run` for LZMA). Min across 3 runs per (algorithm, tier).\n\n");
    md.push_str("The harness enables timing by setting `DOSKRUNCH_BENCH_TIMING=1` in DOS before running the SFX; stubs then write `DKTIME.TXT` (`DECODE_TICKS=<hex>`) into the extraction directory. Tick frequency is ~18.20648 Hz (~54.925 ms/tick).\n\n");
    md.push_str(
        "The benchmark is `#[ignore]`-gated AND env-var-gated (`DOSKRUNCH_RUN_BENCHMARK=1`) so CI's `--ignored` run doesn't silently rewrite this file. Run locally with:\n\n",
    );
    md.push_str("```bash\nDOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy cargo test --test benchmark_tiers -- --ignored --nocapture\n```\n\n");
    md.push_str("| Tier | cputype | aPLib SFX size (bytes) | LZMA SFX size (bytes) | aPLib decode min (ticks) | LZMA decode min (ticks) | LZMA/aPLib ratio | Verdict |\n");
    md.push_str("|------|---------|------------------------|-----------------------|--------------------------|-------------------------|------------------|---------|\n");
    for r in results {
        let ratio = r.lzma_ticks_min as f64 / r.aplib_ticks_min.max(1) as f64;
        let verdict = if ratio <= 10.0 { "met" } else { "not met" };
        md.push_str(&format!(
            "| {tier} | {cputype} | {aplib_sfx} | {lzma_sfx} | {aplib_ticks} (~{aplib_ms:.1} ms) | {lzma_ticks} (~{lzma_ms:.1} ms) | {ratio:.2}× | {verdict} |\n",
            tier = r.tier,
            cputype = r.cputype,
            aplib_sfx = r.aplib_sfx_size,
            lzma_sfx = r.lzma_sfx_size,
            aplib_ticks = r.aplib_ticks_min,
            aplib_ms = ticks_to_ms(r.aplib_ticks_min),
            lzma_ticks = r.lzma_ticks_min,
            lzma_ms = ticks_to_ms(r.lzma_ticks_min),
            ratio = ratio,
            verdict = verdict,
        ));
    }
    md.push_str("\n## Per-run decode ticks\n\n");
    md.push_str("| Tier | aPLib run 1 | aPLib run 2 | aPLib run 3 | LZMA run 1 | LZMA run 2 | LZMA run 3 |\n");
    md.push_str("|------|-------------|-------------|-------------|------------|------------|------------|\n");
    for r in results {
        let a: Vec<String> = (0..RUNS_PER_TIER)
            .map(|i| {
                r.aplib_runs_ticks
                    .get(i)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        let l: Vec<String> = (0..RUNS_PER_TIER)
            .map(|i| {
                r.lzma_runs_ticks
                    .get(i)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        md.push_str(&format!(
            "| {tier} | {} | {} |\n",
            a.join(" | "),
            l.join(" | "),
            tier = r.tier,
        ));
    }
    md.push_str("\n## Gate verdict (PLAN.md §10 Phase 5)\n\n");
    let worst = results
        .iter()
        .map(|r| r.lzma_ticks_min as f64 / r.aplib_ticks_min.max(1) as f64)
        .fold(0.0_f64, f64::max);
    if worst <= 10.0 {
        md.push_str(&format!(
            "Gate met: worst observed LZMA/aPLib decode-only ratio across 386/486/pentium is {worst:.2}× (<= 10×).\n"
        ));
    } else {
        md.push_str(&format!(
            "Gate not met: worst observed LZMA/aPLib decode-only ratio across 386/486/pentium is {worst:.2}× (> 10×).\n"
        ));
    }
    fs::write(&dest, md).expect("write results.md");
    eprintln!("wrote {}", dest.display());
}
