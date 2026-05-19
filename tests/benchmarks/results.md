# Decode-only benchmark (aPLib vs LZMA)

Synthetic mixed-content payload: 500 KiB (512000 bytes) — text + zeros + LCG-random + repeated patterns. See `host/tests/benchmark_tiers.rs::synthesize_payload` for the exact distribution.

Measurement: decode-only BIOS timer ticks (`INT 1Ah, AH=00h`) gathered inside the guest stubs around the depacker call (`aplib_depack` for aPLib and `xz_dec_microlzma_run` for LZMA). Min across 3 runs per (algorithm, tier).

The harness enables timing by setting `DOSKRUNCH_BENCH_TIMING=1` in DOS before running the SFX; stubs then write `DKTIME.TXT` (`DECODE_TICKS=<hex>`) into the extraction directory. Tick frequency is ~18.20648 Hz (~54.925 ms/tick).

The benchmark is `#[ignore]`-gated AND env-var-gated (`DOSKRUNCH_RUN_BENCHMARK=1`) so CI's `--ignored` run doesn't silently rewrite this file. Run locally with:

```bash
DOSKRUNCH_RUN_BENCHMARK=1 SDL_VIDEODRIVER=dummy cargo test --test benchmark_tiers -- --ignored --nocapture
```

| Tier | cputype | aPLib SFX size (bytes) | LZMA SFX size (bytes) | aPLib decode min (ticks) | LZMA decode min (ticks) | LZMA/aPLib ratio | Verdict |
|------|---------|------------------------|-----------------------|--------------------------|-------------------------|------------------|---------|
| 386 | 386 | 156330 | 155116 | 13 (~714.0 ms) | 915 (~50256.8 ms) | 70.38× | not met |
| 486 | 486 | 156330 | 155116 | 13 (~714.0 ms) | 915 (~50256.8 ms) | 70.38× | not met |
| pentium | pentium | 156378 | 155116 | 11 (~604.2 ms) | 915 (~50256.8 ms) | 83.18× | not met |

## Per-run decode ticks

| Tier | aPLib run 1 | aPLib run 2 | aPLib run 3 | LZMA run 1 | LZMA run 2 | LZMA run 3 |
|------|-------------|-------------|-------------|------------|------------|------------|
| 386 | 13 | 13 | 13 | 915 | 915 | 915 |
| 486 | 13 | 13 | 13 | 915 | 915 | 915 |
| pentium | 11 | 11 | 11 | 915 | 915 | 915 |

## Gate verdict (PLAN.md §10 Phase 5)

Gate not met: worst observed LZMA/aPLib decode-only ratio across 386/486/pentium is 83.18× (> 10×).
