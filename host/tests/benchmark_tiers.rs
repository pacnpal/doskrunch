//! Phase 3 §10 timing harness: measure per-tier aPLib decode time for a
//! 500 KiB synthetic mixed-content payload under headless DOSBox-X and
//! regenerate `tests/benchmarks/results.md`.
//!
//! NOT a correctness gate — this file is purely a measurement tool. The
//! large-payload multi-chunk correctness gate lives in
//! `host/tests/dosbox_aplib_large.rs` (runs in CI under `--ignored`,
//! asserts byte-identical extraction at every tier). PLAN.md §10's
//! 2–4× / 5–10× speedup ratios are reported in
//! `tests/benchmarks/results.md` and summarized in `tasks/todo.md`.
//! The harness here reports both:
//!   * isolated decode time (`INT 1Ah` BIOS ticks around `aplib_depack`,
//!     emitted into `DKPERF.BIN` as little-endian `u32` by the bench-only
//!     stub blob built with `make bench`, swapped onto the archive here), and
//!   * end-to-end SFX wall-clock.
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
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

mod common;
use common::{locate_case_insensitive, repo_root, wait_with_timeout, WaitError};

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

/// Smaller payload for `benchmark_lzma_vs_aplib`. LZMA decode under 386
/// emulation is slow, and the bench stub repeats each chunk's decode 8x;
/// at the full 500 KiB a single 386 run blows the 300s DOSBox-X timeout.
/// 64 KiB still spends seconds in the decoder (hundreds of INT 1Ah ticks,
/// fine resolution) while finishing comfortably under the timeout.
const LZMA_BENCH_PAYLOAD_SIZE: usize = 64 * 1024;

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
    synthesize_payload_sized(PAYLOAD_SIZE)
}

/// Same deterministic mixed content as `synthesize_payload`, at an
/// arbitrary `size`. The content cycles through all four types every
/// 1 KiB, so any size ≥ 1 KiB is still "mixed". Used by
/// `benchmark_lzma_vs_aplib`, which needs a much smaller payload than the
/// 500 KiB tier benchmark (see that test for why).
fn synthesize_payload_sized(size: usize) -> Vec<u8> {
    let text = b"doskrunch phase 3 benchmark payload\n";
    let mut out = Vec::with_capacity(size);
    let mut lcg: u32 = 0xDECAFBAD;
    while out.len() < size {
        // 256 bytes of ASCII text
        for i in 0..256 {
            out.push(text[i % text.len()]);
            if out.len() == size {
                return out;
            }
        }
        // 256 bytes of zeros
        let zeros_take = (size - out.len()).min(256);
        out.resize(out.len() + zeros_take, 0);
        if out.len() == size {
            return out;
        }
        // 256 bytes of pseudo-random
        for _ in 0..256 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            out.push((lcg >> 16) as u8);
            if out.len() == size {
                return out;
            }
        }
        // 256 bytes of 16-byte pattern runs
        let pat_idx = (out.len() / 256) as u8;
        for _ in 0..16 {
            for _ in 0..16 {
                out.push(b'A'.wrapping_add(pat_idx % 26));
                if out.len() == size {
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
    decode_ticks_min: u32,
    decode_ticks_runs: Vec<u32>,
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
    // (tier, cputype, bench blob). The bench blob is the same tier built
    // with -DDKRUNCH_BENCH_TICKS (`make bench`); it emits DKPERF.BIN with
    // the INT 1Ah decode-tick total. We pack with the shipped stub (so the
    // reported SFX size is the real product size), then swap the bench
    // blob onto that archive for the timed run — the shipped blobs carry
    // no instrumentation, so the measurement must come from a bench blob.
    let tiers: &[(&str, &str, &str)] = &[
        ("8086", "8086", "aplib_8086_bench.bin"),
        ("386", "386", "aplib_386_bench.bin"),
        ("pentium", "pentium", "aplib_pentium_bench.bin"),
    ];
    let blobs_dir = root.join("stubs").join("blobs");

    let mut results: Vec<TierResult> = Vec::new();
    for (tier, cputype, bench_blob_name) in tiers {
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

        // Build the instrumented SFX: the shipped archive bytes on top of
        // the bench blob (which emits DKPERF.BIN). Requires `make bench`.
        let bench_blob_path = blobs_dir.join(bench_blob_name);
        assert!(
            bench_blob_path.exists(),
            "bench blob {bench_blob_name} not found — build it first:\n  \
             docker run --rm -v \"$PWD:/work\" -w /work/stubs doskrunch-watcom make bench"
        );
        let bench_sfx_path = work_path.join(format!("BENCH_{tier}.EXE"));
        build_bench_sfx(&sfx_path, &bench_blob_path, &bench_sfx_path);

        // Two runs per iteration, deliberately on different blobs:
        //   * wall-clock is measured on the PRODUCTION (shipped) SFX, so it
        //     reflects real user-visible extraction time. Running the bench
        //     blob here would inflate it (8x repeat-decode + DKPERF write).
        //   * decode ticks come from the BENCH SFX (the only blob that emits
        //     DKPERF.BIN). Both runs verify the extracted payload.
        // `run_sfx` sets up an isolated run-dir, runs DOSBox-X at this tier's
        // cputype, asserts a clean exit, and returns (elapsed_ms, run-dir).
        let run_sfx = |sfx_src: &Path, run: usize| -> (u128, tempfile::TempDir) {
            let rundir = tempfile::tempdir().expect("rundir");
            let rundir_path = rundir.path();
            fs::copy(sfx_src, rundir_path.join("OUT.EXE")).expect("copy sfx into rundir");
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
            match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
                Ok(s) => assert!(s.success(), "dosbox-x non-zero (tier {tier} run {run})"),
                Err(WaitError::Timeout) => {
                    panic!("dosbox-x timeout {DOSBOX_TIMEOUT:?} (tier {tier} run {run})")
                }
                Err(WaitError::Wait(e)) => {
                    panic!("dosbox-x wait error: {e} (tier {tier} run {run})")
                }
            }
            (started.elapsed().as_millis(), rundir)
        };

        let verify_extracted = |rundir_path: &Path, tier: &str, run: usize| {
            let extracted = locate_case_insensitive(rundir_path, "BENCH_PAYLOAD.BIN")
                .or_else(|| locate_case_insensitive(rundir_path, "BENCH_PA.BIN"))
                .expect("locate extracted payload");
            let body = fs::read(&extracted).expect("read extracted");
            assert!(
                body.as_slice() == payload.as_slice(),
                "extracted payload mismatch (tier {tier} run {run})"
            );
        };

        let mut runs_ms: Vec<u128> = Vec::with_capacity(RUNS_PER_TIER);
        let mut decode_ticks_runs: Vec<u32> = Vec::with_capacity(RUNS_PER_TIER);
        for run in 0..RUNS_PER_TIER {
            // Production SFX → wall-clock + correctness.
            let (elapsed, prod_dir) = run_sfx(&sfx_path, run);
            runs_ms.push(elapsed);
            verify_extracted(prod_dir.path(), tier, run);

            // Bench SFX → isolated decode ticks + correctness.
            let (_bench_ms, bench_dir) = run_sfx(&bench_sfx_path, run);
            decode_ticks_runs.push(read_perf_ticks_file(bench_dir.path(), tier, run));
            verify_extracted(bench_dir.path(), tier, run);
        }
        let decode_ticks_min = *decode_ticks_runs.iter().min().unwrap();
        let wall_clock_min_ms = *runs_ms.iter().min().unwrap();
        results.push(TierResult {
            tier,
            cputype,
            sfx_size,
            decode_ticks_min,
            decode_ticks_runs,
            wall_clock_min_ms,
            runs: runs_ms,
        });
    }

    write_results_markdown(&root, &results, &payload);

    // Print the ratios for inspection. This is a measurement harness,
    // not a gate — the byte-identical correctness check lives in
    // dosbox_aplib_large.rs (and runs in CI under --ignored without
    // requiring the env var). The PLAN.md §10 2–4× / 5–10× speedup
    // expectation is reported in results.md but this harness still
    // doesn't assert on the ratio. The numbers are empirical data, not
    // a deterministic correctness condition.
    let baseline_ticks = results
        .iter()
        .find(|r| r.tier == "8086")
        .map(|r| r.decode_ticks_min.max(1))
        .unwrap_or(1);
    let baseline_wall = results
        .iter()
        .find(|r| r.tier == "8086")
        .map(|r| r.wall_clock_min_ms.max(1))
        .unwrap_or(1);
    for r in &results {
        let decode_ratio = baseline_ticks as f64 / r.decode_ticks_min.max(1) as f64;
        let wall_ratio = baseline_wall as f64 / r.wall_clock_min_ms.max(1) as f64;
        println!(
            "tier={tier:8} decode={ticks:6} ticks ({decode_ratio:.2}x)  wall={ms:6} ms ({wall_ratio:.2}x)",
            tier = r.tier,
            ticks = r.decode_ticks_min,
            decode_ratio = decode_ratio,
            ms = r.wall_clock_min_ms,
            wall_ratio = wall_ratio,
        );
    }
}

/// Pack `payload_path` with `algo` at `tier`, swap in `bench_blob_name` (an
/// INT 1Ah-instrumented `make bench` blob), run it under DOSBox-X at
/// `cputype`, and return the smallest DKPERF.BIN decode-tick count over
/// RUNS_PER_TIER runs. Shared by `benchmark_lzma_vs_aplib`.
#[allow(clippy::too_many_arguments)]
fn measure_decode_ticks(
    bin: &str,
    payload_path: &Path,
    payload: &[u8],
    blobs_dir: &Path,
    work_path: &Path,
    algo: &str,
    tier: &str,
    cputype: &str,
    bench_blob_name: &str,
) -> u32 {
    let sfx_path = work_path.join(format!("OUT_{algo}_{tier}.EXE"));
    let status = Command::new(bin)
        .arg("pack")
        .arg(&sfx_path)
        .arg(payload_path)
        .args(["--algo", algo, "--target", tier])
        .status()
        .expect("spawn doskrunch pack");
    assert!(status.success(), "pack failed for {algo}/{tier}");

    let bench_blob_path = blobs_dir.join(bench_blob_name);
    assert!(
        bench_blob_path.exists(),
        "bench blob {bench_blob_name} not found — build it first:\n  \
         docker run --rm -v \"$PWD:/work\" -w /work/stubs doskrunch-watcom make bench"
    );
    let bench_sfx_path = work_path.join(format!("BENCH_{algo}_{tier}.EXE"));
    build_bench_sfx(&sfx_path, &bench_blob_path, &bench_sfx_path);

    let mut min_ticks = u32::MAX;
    for run in 0..RUNS_PER_TIER {
        let rundir = tempfile::tempdir().expect("rundir");
        let rundir_path = rundir.path();
        fs::copy(&bench_sfx_path, rundir_path.join("OUT.EXE")).expect("copy sfx into rundir");

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

        let mut dosbox = Command::new("dosbox-x")
            .arg("-conf")
            .arg(&conf_path)
            .arg("-exit")
            .arg("-nogui")
            .env("SDL_VIDEODRIVER", "dummy")
            .spawn()
            .expect("spawn dosbox-x");
        match wait_with_timeout(&mut dosbox, DOSBOX_TIMEOUT) {
            Ok(s) => assert!(s.success(), "dosbox-x non-zero ({algo}/{tier} run {run})"),
            Err(WaitError::Timeout) => panic!("dosbox-x timeout ({algo}/{tier} run {run})"),
            Err(WaitError::Wait(e)) => panic!("dosbox-x wait error: {e} ({algo}/{tier} run {run})"),
        }

        // Verify the extracted payload before trusting the timing: a bench
        // blob that decodes to wrong bytes but still exits 0 would otherwise
        // yield misleading ticks. Mirrors benchmark_tier_decompression.
        let extracted = locate_case_insensitive(rundir_path, "BENCH_PAYLOAD.BIN")
            .or_else(|| locate_case_insensitive(rundir_path, "BENCH_PA.BIN"))
            .unwrap_or_else(|| panic!("extracted payload not found ({algo}/{tier} run {run})"));
        let body = fs::read(&extracted).expect("read extracted");
        assert!(
            body.as_slice() == payload,
            "extracted payload mismatch ({algo}/{tier} run {run})"
        );

        let ticks = read_perf_ticks_file(rundir_path, tier, run);
        if ticks < min_ticks {
            min_ticks = ticks;
        }
    }
    min_ticks
}

/// LZMA-vs-aPLib isolated decode-tick comparison (folded in from the closed
/// PR #18 / issue #13). For each tier that has both an aplib and an lzma
/// `make bench` blob (LZMA is 386+), pack the same payload with each algo,
/// swap in the matching INT 1Ah bench blob, and report the lzma/aplib decode
/// ratio. PLAN.md §10 frames the gate as "LZMA decode within ~10x aPLib on
/// the same payload". Measurement only — no assertion (DOSBox-X is a noisy
/// substrate for absolute timing; see tests/benchmarks/results.md).
#[test]
#[ignore = "needs dosbox-x AND DOSKRUNCH_RUN_BENCHMARK=1 AND bench blobs from `make bench`"]
fn benchmark_lzma_vs_aplib() {
    if std::env::var_os("DOSKRUNCH_RUN_BENCHMARK").is_none() {
        eprintln!("benchmark_lzma_vs_aplib: skipped (set DOSKRUNCH_RUN_BENCHMARK=1 to run)");
        return;
    }

    let root = repo_root();
    let work = tempfile::tempdir().expect("create tempdir");
    let work_path = work.path();
    let blobs_dir = root.join("stubs").join("blobs");
    let bin = env!("CARGO_BIN_EXE_doskrunch");

    let payload = synthesize_payload_sized(LZMA_BENCH_PAYLOAD_SIZE);
    assert_eq!(payload.len(), LZMA_BENCH_PAYLOAD_SIZE);
    // Write into this test's own tempdir (not target/) so it can't race
    // benchmark_tier_decompression's 500 KiB bench_payload.bin under cargo's
    // parallel test runner. The file MUST be named bench_payload.bin so its
    // 8.3 form is BENCH_PA.BIN — the marker the bench stub looks for.
    let payload_path = work_path.join("bench_payload.bin");
    fs::write(&payload_path, &payload).expect("write payload");

    // Only tiers with BOTH a `make bench` aplib and lzma blob (LZMA is 386+).
    let tiers: &[(&str, &str, &str, &str)] = &[
        ("386", "386", "aplib_386_bench.bin", "lzma_386_bench.bin"),
        (
            "pentium",
            "pentium",
            "aplib_pentium_bench.bin",
            "lzma_pentium_bench.bin",
        ),
    ];

    println!("\nLZMA vs aPLib decode ticks (INT 1Ah, min of {RUNS_PER_TIER} runs):");
    for (tier, cputype, aplib_blob, lzma_blob) in tiers {
        let aplib_ticks = measure_decode_ticks(
            bin,
            &payload_path,
            &payload,
            &blobs_dir,
            work_path,
            "aplib",
            tier,
            cputype,
            aplib_blob,
        );
        let lzma_ticks = measure_decode_ticks(
            bin,
            &payload_path,
            &payload,
            &blobs_dir,
            work_path,
            "lzma",
            tier,
            cputype,
            lzma_blob,
        );
        let ratio = lzma_ticks as f64 / aplib_ticks.max(1) as f64;
        println!(
            "  tier={tier:8} aplib={aplib_ticks:6} ticks  lzma={lzma_ticks:6} ticks  lzma/aplib={ratio:.2}x",
        );
    }
    println!(
        "  (PLAN.md §10 gate context: LZMA decode within ~10x aPLib on the same payload.)"
    );
    // No assertion — measurement harness only; record numbers by hand.
}

fn write_results_markdown(root: &Path, results: &[TierResult], payload: &[u8]) {
    let dest = root.join("tests").join("benchmarks").join("results.md");
    fs::create_dir_all(dest.parent().unwrap()).expect("mkdir benchmarks");

    let baseline_ticks = results
        .iter()
        .find(|r| r.tier == "8086")
        .map(|r| r.decode_ticks_min.max(1))
        .unwrap_or(1);
    let baseline_wall = results
        .iter()
        .find(|r| r.tier == "8086")
        .map(|r| r.wall_clock_min_ms.max(1))
        .unwrap_or(1);
    let ratio_386 = results
        .iter()
        .find(|r| r.tier == "386")
        .map(|r| baseline_ticks as f64 / r.decode_ticks_min.max(1) as f64);
    let ratio_pentium = results
        .iter()
        .find(|r| r.tier == "pentium")
        .map(|r| baseline_ticks as f64 / r.decode_ticks_min.max(1) as f64);
    let gate_met = ratio_386
        .map(|v| (2.0..=4.0).contains(&v))
        .unwrap_or(false)
        && ratio_pentium
            .map(|v| (5.0..=10.0).contains(&v))
            .unwrap_or(false);

    let mut md = String::new();
    md.push_str("# Tier decompression benchmark\n\n");
    md.push_str(&format!(
        "Synthetic mixed-content payload: {} KiB ({} bytes) — text + zeros + LCG-random + repeated patterns. \
        See `host/tests/benchmark_tiers.rs::synthesize_payload` for the exact distribution.\n\n",
        payload.len() / 1024,
        payload.len(),
    ));
    md.push_str("Measurement setup:\n\n");
    md.push_str("* Isolated decode time: a bench-only stub blob (built with `make bench`, never shipped) wraps each depack call (`aplib_depack` / `lzsa2_depack`, or the LZMA decode in the lzma stub) with `INT 1Ah` (`AH=00h`). When the `BENCH_PA` marker payload is present it repeats each chunk's decode 8× to overcome `INT 1Ah`'s coarse resolution, and writes the **accumulated** ticks across all chunks and repeats to `DKPERF.BIN` (little-endian `u32`). So DKPERF values are sums, not per-call times — only ratios between tiers/algos are meaningful. The harness swaps this blob onto the packed archive for the timed run, so the shipped stubs carry no instrumentation. `INT 1Ah` ticks run at ~18.2 Hz and are available on every target tier (8086+).\n");
    md.push_str("* End-to-end wall-clock: host-side timer around the full DOSBox run (`cycles=auto`), min across 3 runs, measured on the **shipped** (uninstrumented) SFX — a separate run from the decode-tick measurement so the 8x repeat-decode never inflates it.\n");
    md.push_str("* Benchmark gating: `#[ignore]` AND env-var-gated (`DOSKRUNCH_RUN_BENCHMARK=1`) so CI's `--ignored` run doesn't silently rewrite this file.\n\n");
    md.push_str("```bash\nDOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy cargo test --test benchmark_tiers -- --ignored --nocapture\n```\n\n");
    md.push_str("## Isolated aPLib decode time (INT 1Ah ticks)\n\n");
    md.push_str("| Tier | cputype | SFX size (bytes) | Decode min (ticks) | Decode ratio vs 8086 |\n");
    md.push_str("|------|---------|------------------|---------------------|----------------------|\n");
    for r in results {
        let ratio = baseline_ticks as f64 / r.decode_ticks_min.max(1) as f64;
        md.push_str(&format!(
            "| {tier} | {cputype} | {sfx} | {ticks} | {ratio:.2}× |\n",
            tier = r.tier,
            cputype = r.cputype,
            sfx = r.sfx_size,
            ticks = r.decode_ticks_min,
            ratio = ratio,
        ));
    }
    md.push_str("\n");
    if let (Some(r386), Some(rp5)) = (ratio_386, ratio_pentium) {
        md.push_str(&format!(
            "* Phase 3 gate check (decode-only speedup vs 8086, computed as 8086_ticks / tier_ticks; >1 = faster): 386 = **{r386:.2}×**, pentium = **{rp5:.2}×** → **{verdict}**.\n",
            verdict = if gate_met { "gate met" } else { "gate not met" }
        ));
    }
    md.push_str("\n## End-to-end wall-clock (DOSBox-X cycles=auto)\n\n");
    md.push_str("| Tier | Wall clock min (ms) | Ratio vs 8086 |\n");
    md.push_str("|------|----------------------|----------------|\n");
    for r in results {
        let ratio = baseline_wall as f64 / r.wall_clock_min_ms.max(1) as f64;
        md.push_str(&format!(
            "| {tier} | {ms} | {ratio:.2}× |\n",
            tier = r.tier,
            ms = r.wall_clock_min_ms,
            ratio = ratio,
        ));
    }
    md.push_str("\n## Per-run detail\n\n");
    md.push_str("| Tier | Decode run 1 (ticks) | Decode run 2 (ticks) | Decode run 3 (ticks) | Wall run 1 (ms) | Wall run 2 (ms) | Wall run 3 (ms) |\n");
    md.push_str("|------|----------------------|----------------------|----------------------|-----------------|-----------------|-----------------|\n");
    for r in results {
        let decode_cells: Vec<String> = (0..RUNS_PER_TIER)
            .map(|i| {
                r.decode_ticks_runs
                    .get(i)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        let wall_cells: Vec<String> = (0..RUNS_PER_TIER)
            .map(|i| r.runs.get(i).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        md.push_str(&format!(
            "| {tier} | {} | {} |\n",
            decode_cells.join(" | "),
            wall_cells.join(" | "),
            tier = r.tier,
        ));
    }
    md.push_str(
        "\n## Caveats\n\n\
         * `INT 1Ah` has coarse resolution (~54.9 ms/tick), so small payloads are noisy. \
           The 500 KiB payload keeps decode long enough to make ratios stable.\n\
         * `INT 1Ah` wraps at MIDNIGHT (0x1800B0 ticks), not at 2^32, so a plain `t1 - t0` is \
           wrong across a rollover. The stub computes a wrap-correct delta (`tick_delta`); a \
           single sub-second decode crossing midnight is astronomically unlikely, but the \
           arithmetic is correct regardless.\n\
         * Wall-clock and decode-time are both useful: the decode ratio tracks `aplib_depack` \
           itself (from the bench SFX); wall-clock is measured on a separate run of the shipped \
           SFX, so it reflects real user-visible elapsed time including DOS startup and \
           INT 21h file I/O.\n\
         * The benchmark is double-gated (`#[ignore]` + `DOSKRUNCH_RUN_BENCHMARK=1`); CI never \
           sets `DOSKRUNCH_RUN_BENCHMARK=1`, so its `--ignored` run skips this test via the \
           env-var check and this file is only regenerated by a local opt-in run. Commit a \
           refreshed copy when the numbers move.\n",
    );
    fs::write(&dest, md).expect("write results.md");
    eprintln!("wrote {}", dest.display());
}

/// Build an instrumented bench SFX: the shipped archive bytes (read from
/// `prod_sfx`) appended to `bench_blob`, with the trailing DKTR archive
/// offset rewritten to the bench blob length. The stub finds the DKCH
/// archive via EOF-8 -> archive_offset, so after swapping in a different-
/// sized stub the offset must point at the new (post-blob) archive start.
fn build_bench_sfx(prod_sfx: &Path, bench_blob: &Path, out: &Path) {
    let prod = fs::read(prod_sfx).expect("read prod sfx");
    assert!(prod.len() >= 8, "sfx too short for trailer");
    let tstart = prod.len() - 8;
    assert_eq!(
        &prod[tstart..tstart + 4],
        b"DKTR",
        "production SFX trailer magic is not DKTR"
    );
    let archive_off =
        u32::from_le_bytes(prod[tstart + 4..tstart + 8].try_into().unwrap()) as usize;

    let blob = fs::read(bench_blob).expect("read bench blob");
    let blob_len = blob.len();
    let mut sfx = blob;
    sfx.extend_from_slice(&prod[archive_off..]);
    let n = sfx.len();
    sfx[n - 4..n].copy_from_slice(&(blob_len as u32).to_le_bytes());
    fs::write(out, &sfx).expect("write bench sfx");
}

fn read_perf_ticks_file(rundir: &Path, tier: &str, run: usize) -> u32 {
    let perf = locate_case_insensitive(rundir, "DKPERF.BIN")
        .unwrap_or_else(|| panic!("missing DKPERF.BIN (tier {tier} run {run})"));
    let bytes = fs::read(&perf).unwrap_or_else(|e| {
        panic!(
            "failed to read {} (tier {tier} run {run}): {e}",
            perf.display()
        )
    });
    if bytes.len() < 4 {
        panic!(
            "short DKPERF.BIN ({} bytes) (tier {tier} run {run})",
            bytes.len()
        );
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
