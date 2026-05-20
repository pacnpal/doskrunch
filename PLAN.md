# doskrunch: DOS Self-Extracting EXE Builder

A cross-platform CLI that produces self-extracting DOS .EXE/.COM files from arbitrary input files, using modern compression algorithms with hand-tuned decompressors for every x86 CPU tier from the original 8088 up to Pentium III.

## 0. Clarifying "8-bit or 16-bit"

DOS itself is 16-bit real mode. There is no native 8-bit DOS. What people usually mean by "8-bit" in this context is one of three things:

- **8088-compatible code** (no 286+ instructions). The 8088 has an 8-bit external data bus, so people sometimes call it the "8-bit DOS target." This is the broadest compatibility tier and runs on the original 1981 IBM PC.
- **DOS .COM files**, single segment, max ~64KB. Closest thing to an "8-bit feel" in the DOS world.
- **CP/M-80 / Z80**. A different OS entirely. Ruled out unless we explicitly want to target it.

This project targets **DOS 16-bit real mode**, produces both **.EXE (MZ format)** and **.COM** outputs, and supports CPU tiers from **8086/8088** up through **Pentium III**. All tiers stay in 16-bit real mode. We do not ship a DOS extender, DPMI host, or protected-mode code.

## 1. Goals

Build a single cross-platform CLI (Windows, macOS, Linux, x86_64 and ARM) that takes a set of files and produces a DOS .EXE (or .COM) that, when run on DOS, extracts those files to the current directory. Optionally run a configured command afterward.

Hard requirements:

- Output must run on real 8088/8086 by default.
- Optional optimized builds for 386, 486, Pentium, Pentium-MMX, Pentium II, Pentium III.
- Best practical compression for the file types people actually ship (text, binaries, mixed).
- Small decompressor stub. Every byte of stub eats into compression savings.
- No "needs a DOS machine to build." Builds fully on modern hosts.
- Single static binary. No Python interpreter, no runtime dependencies.
- Reproducible builds. Same input bytes produce the same output bytes.

Soft goals:

- Optional "extract and run" mode for installer-style use.
- Optional encryption (cheap to bolt on later, not in v1).

## 2. CPU target tiers

The SFX must support multiple CPU targets. Each tier ships as a separate pre-built stub binary. The host tool selects the right blob at pack time based on a `--target` flag.

Tiers, broadest to tightest:

| Tier | Watcom flag | Adds over previous tier | Use case |
|---|---|---|---|
| `8086` (default) | `-0` | Baseline 16-bit, no 286+ instructions | Original IBM PC and clones. Maximum compatibility. |
| `286` | `-2` | PUSHA/POPA, IMUL imm, SHL reg,imm8 | Marginal gain over 8086. Mostly historical. |
| `386` | `-3` | 32-bit registers in real mode via operand-size prefix (0x66) | Big jump. 3-5x faster LZ copy loops. Recommended default for retro hardware from 1986 forward. |
| `486` | `-4` | BSWAP, single-cycle most instructions, better cache | Incremental over 386. Useful for tight inner loops. |
| `pentium` | `-5` | U/V pipeline pairing, CMPXCHG8B, RDTSC | Hand-scheduled ASM decoders pair instructions across both pipes. |
| `pentium-mmx` | `-5` + MMX | P55C with MMX baseline | MMX registers usable for vectorized memory copy. |
| `p2` | `-6` | Pentium II, P6 out-of-order, MMX assumed | P6 reorders instructions for you. Decoders can be less hand-scheduled. |
| `p3` | `-6` + SSE | Pentium III, SSE available | SSE vectorized copy paths in the decoder. |

All tiers stay in **16-bit real mode**. The 386+ tiers use the operand-size prefix (0x66) to access 32-bit registers from real mode. This is supported by every 386 and later in real mode, no extender required.

For v1 ship three tiers: **`8086`**, **`386`**, **`pentium`**. These three cover the meaningful inflection points (no 32-bit math, full 32-bit math, paired pipelines). The other five (`286`, `486`, `pentium-mmx`, `p2`, `p3`) get implemented in Phase 5 once the pipeline is solid.

Default `--target` is `8086`. Default `--algo` is `aplib`.

## 3. Compression algorithm survey

Decompressor size is the size of code that has to ship inside every SFX. Memory is decompressor working memory on top of dictionary.

| Algorithm | Ratio | Decomp size (x86) | Decomp RAM | 8088 OK | Modern OSS compressor | Notes |
|---|---|---|---|---|---|---|
| **aPLib** | Very good (often beats PKZIP) | ~169 bytes (size-opt) to ~250 bytes (fast) | None beyond dict | Yes | **apultra** (5-7% better than appack, zlib-style license) | Designed exactly for this. Same family as aPACK, the long-standing DOS exe packer. 16-bit and 32-bit decoders both exist. |
| **LZSA2** | Good | ~200-300 bytes on 8088 | None beyond dict | Yes (hand-tuned 8088 decoder by Jim Leonard / Trixter) | **lzsa** (Emmanuel Marty, zlib-style license) | Tuned for very fast decompression on retro CPUs. |
| **LZ4** | Lower than aPLib/LZSA2 | Tiny, very fast | None beyond dict | Yes (LZ4_8088 by Trixter) | Yes, LZ4 reference | Fastest decomp, weakest ratio. Useful for low-RAM speed-critical cases. |
| **UCL (NRV)** | Similar to aPLib | "few hundred bytes" | None | Yes (UPX uses this with --8086) | UPX/UCL by Oberhumer | What UPX uses for DOS by default. GPL with linking exception. |
| **Deflate** | Decent | Several KB | ~32KB window | Yes | zlib, miniz, tinf | Universal, but stub overhead is real. Info-ZIP's DOS unzipsfx is ~30KB. |
| **LZMA** | Best | ~3-5KB (UPX), ~10KB+ (full xz embedded) | ~100KB+ for 64KB dict, more for larger | 386+ recommended, very slow on 8088 (UPX warns 30x slower than NRV) | xz-embedded, LZMA SDK | Best ratio but biggest stub and biggest RAM. UPX has it for dos/exe behind explicit --lzma. |

What gets implemented:

- **Default: aPLib** via **apultra** as the modern open-source compressor. Hits the sweet spot of ratio, decompressor size, and 8088 compatibility. 169-byte decompressor has shipped in production code since 1998.
- **Secondary: LZSA2** as a "fast decompression" mode for users who care about how fast the SFX feels on a 4.77MHz 8088. Jim Leonard wrote both the LZ4_8088 and LZSA decompressors, so the 8088 implementations are about as fast as physically possible.
- **Tertiary: LZMA** as a "best ratio, 386+ only" mode. Useful for shipping large datasets where the user has modern retro hardware.

Skipping Deflate. More code than aPLib for worse ratios on the sizes most people ship as DOS SFXs. Reconsider for v2 if ZIP-compatibility becomes desirable.

## 4. Algorithm × target matrix

The user picks both algorithm and target. The matrix:

| | 8086 | 286 | 386 | 486 | pentium | pentium-mmx | p2 | p3 |
|---|---|---|---|---|---|---|---|---|
| stored | yes | yes | yes | yes | yes | yes | yes | yes |
| aplib | yes (16-bit decoder) | yes | yes (32-bit decoder) | yes | yes (paired) | yes | yes | yes |
| lzsa2 | yes (8088 decoder) | yes | yes (32-bit decoder) | yes | yes (paired) | yes | yes | yes |
| lzma | **no** | **no** | yes | yes | yes | yes | yes | yes |

The host tool rejects `--algo lzma --target 8086` and `--algo lzma --target 286` with a clear error: "LZMA requires 386 or later; pick --target 386 or higher, or use --algo aplib."

Recommendations the README will state plainly:

- "I don't know what to pick" → `--algo aplib --target 8086` (the default).
- "Modern retro hardware (386 onward)" → `--algo aplib --target 386`.
- "Late 90s DOS gaming rig" → `--algo lzma --target p2`.
- "Want the SFX to feel snappy on a 4.77MHz PC" → `--algo lzsa2 --target 8086`.

## 5. Output formats

Two output modes:

- **.COM**: single segment, code+data+heap all under 64KB. Use when uncompressed payload + decompressor + working buffers fit comfortably. Loader is trivial (DOS just loads at CS:0100h).
- **.EXE (MZ)**: standard DOS 16-bit executable with MZ header. Required when payload is large. Supports relocations, multiple segments, larger uncompressed sizes.

Practical limits inside DOS:

- Conventional memory ceiling is ~640KB minus DOS, drivers, environment. Realistic free conventional memory is 500-580KB on a clean DOS box.
- For payloads larger than that, we need to extract a chunk, write to disk, free the buffer, decode the next chunk. So the format needs to be chunked, not one giant compressed blob.
- File system writes go through INT 21h, so we're not bound by RAM for the total payload, only for the working buffer.

The archive container inside the SFX is **chunk-streamed**: for each file, write a small per-file header (name, size, CRC, attributes, optional timestamp), then one or more compressed chunks. Each chunk decompresses into a fixed-size buffer (32KB or 48KB) and gets flushed to disk before the next chunk loads. RAM use stays bounded; payload size does not.

Filename handling on DOS:

- FAT 8.3 only in plain real-mode DOS. Long file names need INT 21h/71xxh which only works under Win9x DOS box, not pure DOS.
- Host tool stores 8.3 names (mangled if source is longer) plus optional LFN. Stub uses 8.3 always in v1. Skip LFN INT 21h calls; they're not portable across DOS variants.

## 6. DOS stub design

The stub does this on every invocation:

1. Locate itself on disk (read argv[0] / PSP environment to get its own path).
2. Open itself, seek to the archive offset (a fixed offset stored in the stub or right after the MZ header).
3. Parse archive header (magic, version, file count, total size, CRC).
4. For each file: read per-file header, create the output file via INT 21h/3Ch, loop over chunks, decompress each chunk into RAM, write to disk via INT 21h/40h.
5. Optionally invoke a configured command via INT 21h/4Bh.

Memory model:

- **Small model** (CS=DS=SS, one code seg, one data seg, both ≤64KB). Easiest. Use this if the stub fits.
- **Compact model** (one code, multiple data) only if needed.
- The compressed payload itself doesn't need to be in memory all at once. We read it in chunks from the EXE on disk via DOS file handles.

CPU-tier-specific design:

- **8086/286 tier**: 16-bit registers only. NASM `CPU 8086` (or `CPU 286`) directive enforces no newer instructions. aPLib reference 16-bit decompressor used directly.
- **386/486 tier**: 16-bit real mode, but inner loops use operand-size prefix (0x66) to access 32-bit registers. aPLib reference 32-bit decompressor used. NASM `CPU 386` directive.
- **pentium tier**: 32-bit decoder hand-scheduled for U/V pipeline pairing. Each comment-annotated instruction notes whether it pairs in U or V pipe. NASM `CPU pentium`.
- **pentium-mmx**: MMX registers used for the 8-byte memory copy loop. Cuts memcpy time roughly in half over scalar 32-bit copies.
- **p2/p3**: P6 OoO scheduling means we don't need to hand-schedule pairing as aggressively. SSE in p3 lets us do 16-byte aligned copies for large literal runs.

Stub language:

- **C with Open Watcom v2** for the housekeeping (argv parsing, archive iteration, INT 21h wrappers, error handling). Open Watcom v2 cross-compiles 16-bit DOS from modern Linux, macOS, and Windows hosts. CMake support in CMake 3.18+. Maintained Debian-based Docker image for CI.
- **Hand-tuned NASM (or JWasm) assembly for the decompressor inner loop.** This is where the small decompressor lives. Don't try to write it in C; the reference assembly is already optimal.

Stub size budgets per tier:

| Tier | Target | Hard ceiling |
|---|---|---|
| 8086 | 4KB | 8KB |
| 286 | 4KB | 8KB |
| 386 | 6KB | 10KB |
| 486 | 6KB | 10KB |
| pentium | 8KB | 12KB |
| pentium-mmx | 8KB | 12KB |
| p2 | 8KB | 12KB |
| p3 | 10KB | 14KB |

Higher tiers carry more decoder variants (e.g., MMX/SSE copy paths) and slightly more elaborate scheduling, so the budget rises. Still better than Info-ZIP's DOS unzipsfx (25-40KB) because we ship one algorithm and one container format with no legacy compatibility.

## 7. Modern host tool

Language: **Rust**. Single static binary, great cross-compilation, mature CLI ecosystem (clap), reliable distribution via cargo install and GitHub Releases.

The host tool needs:

- An aPLib-compatible compressor. Vendor apultra's C source via git subtree, build with the `cc` crate. If a mature pure-Rust aPLib crate exists at start time, prefer that.
- A way to embed the pre-built stub binaries. Standard approach: build stubs once in CI, commit the resulting `.bin` blobs to `stubs/*.bin`, use `include_bytes!` in Rust to embed.
- File walking, CRC32, archive serialization. Trivial in Rust.
- Output: write the embedded stub, then append the archive, then patch a length/offset field in the stub header so it can find the archive at runtime.

CLI shape:

```
doskrunch pack output.exe file1.txt file2.dat dir/
doskrunch pack output.exe -r --algo aplib --target 386 -o setup.exe src/
doskrunch pack --format com --target 8086 small.com tiny.txt
doskrunch pack --run-after install.bat output.exe *
doskrunch pack --algo lzma --target p2 large.exe payload/
doskrunch inspect output.exe
doskrunch unpack output.exe -d ./extracted
doskrunch list-targets
doskrunch list-algos
```

`inspect` and `unpack` make it possible to read SFXs built by this tool without booting DOS. Critical for testing and for users who want to verify what an SFX contains before running it.

## 8. Archive container format

Fresh design. Not ZIP-compatible in v1.

```
[ MZ header ][ stub code ][ archive header ][ file 1 ][ file 2 ]...[ trailer ]
```

Archive header (fixed offset right after stub):

- Magic: `"DKCH"` (4 bytes, "DOS KrunCH")
- Version: u8
- Algorithm: u8 (0=stored, 1=aplib, 2=lzsa2, 3=lzma)
- Target tier: u8 (informational; stub knows its own tier already)
- Flags: u16 (run-after, encrypted, etc.)
- File count: u16
- Total uncompressed size: u32
- Total compressed size: u32
- Run-after command offset: u16 (0 if none)
- Header CRC32: u32

Per-file record:

- Name length: u8
- Name: 8.3 ASCII, NUL-terminated (or full path with `\` separators if directory mode)
- Attributes: u8 (FAT attribute byte)
- Timestamp: u32 (FAT format dos_date/dos_time)
- Uncompressed size: u32
- Chunk count: u16
- Per-chunk: compressed_size u16, uncompressed_size u16, data
- File CRC32: u32 (covers uncompressed contents)

Trailer at end of file with magic and offset back to archive header. Lets the stub find the archive even if its own size is not known at runtime.

## 9. Build pipeline

Repo layout:

```
doskrunch/
  host/                    # Rust CLI
    src/
      main.rs
      pack.rs
      unpack.rs
      inspect.rs
      archive.rs
      stubs.rs             # include_bytes! for every tier blob
    Cargo.toml
  stubs/
    src/
      stub.c               # main stub logic in Watcom C
      aplib_depack_16.asm  # 16-bit aPLib decoder (8086/286 tiers)
      aplib_depack_32.asm  # 32-bit aPLib decoder (386+ tiers)
      aplib_depack_p5.asm  # pentium U/V-paired variant
      aplib_depack_mmx.asm # MMX-accelerated copy paths
      aplib_depack_sse.asm # SSE-accelerated copy paths
      lzsa2_depack_*.asm   # corresponding LZSA2 variants
      lzma_depack_*.asm    # 386+ only
      dos.h                # INT 21h wrappers
    blobs/
      aplib_8086.bin       # checked-in binary blobs, one per (algo, tier)
      aplib_386.bin
      aplib_pentium.bin
      ...
    Makefile               # wmake
    Dockerfile             # debian-open-watcom base + NASM
  vendor/
    apultra/               # git subtree
    lzsa/                  # git subtree
    xz-embedded/           # git subtree (Phase 5)
  tests/
    fixtures/              # small test payloads
    integration/           # build SFX, run in DOSBox-X, diff output
    benchmarks/            # decompression timing across tiers
    fuzz/                  # cargo-fuzz targets
  .github/workflows/
    build-stubs.yml        # rebuilds stub blobs in CI on demand
    release.yml            # cross-compiles host tool for all platforms
    test.yml               # runs unit + DOSBox-X integration tests
  CLAUDE.md
  PLAN.md
  README.md
```

CI:

- **build-stubs.yml**: pulls the `volkertb/debian-open-watcom` Docker image, builds all stub variants, uploads binaries as artifacts. Run on stub-source changes. Output committed to `stubs/blobs/*.bin`.
- **release.yml**: builds the Rust host tool for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64. Uploads to GitHub Releases.
- **test.yml**: runs unit tests, builds sample SFXs, runs them under DOSBox-X headless across multiple CPU emulation profiles (8086, 386, pentium_mmx), verifies extracted output matches input.

Testing strategy:

1. Unit tests for the archive format encoder/decoder in Rust.
2. Fuzzing the parser with `cargo-fuzz`.
3. Round-trip tests: pack, then unpack with the host tool's `unpack` subcommand, verify byte-identical output.
4. **DOSBox-X headless integration tests**: actually run the SFX inside DOSBox-X in CI on multiple CPU emulation profiles. Capture extracted files via a shared mount, diff against expected. Only way to catch real DOS bugs.
5. Benchmarks: decompression timing across all tiers, recorded in `tests/benchmarks/results.md`. Isolated decode time uses the INT 1Ah BIOS tick counter inside the guest (one mechanism, works 8086+); host wallclock is captured as a cross-check.
6. Manual testing on real hardware. 86Box gives cycle-accurate 8088/8086 emulation. Real iron (e.g., a Pentium III machine) catches things emulators miss.

## 10. Implementation phases

Phases run in strict order. Phase N+1 starts only after Phase N's verification passes.

### Phase 1: store-only SFX, 8086 tier, no compression

Goal: produce an 8086-targeted DOS .EXE that, run under DOSBox-X with 8086 CPU emulation, extracts a fixed set of test files byte-identical to originals using the "stored" algorithm.

Build:

- Rust workspace with `host/` crate. clap CLI: pack, unpack, inspect subcommands.
- `stubs/` directory with a single Watcom C stub compiled with `-0`.
- Dockerfile based on volkertb/debian-open-watcom for stub builds.
- CI workflow that builds the stub blob and commits it.
- Archive container format from section 8, implemented in Rust and parsed in C.

Verify:

- `cargo test` passes.
- Round-trip test: pack a fixture directory, unpack with host tool, diff. Bit-identical.
- DOSBox-X headless integration test with `cpu_type=8086`. Extracted files bit-identical.

### Phase 2: aPLib compression, 8086 tier

Goal: phase 1 plus aPLib compression.

Build:

- Vendor apultra under `vendor/apultra/` via git subtree. Build via `cc` crate. Confirm license compatibility before committing.
- Port reference 16-bit aPLib decompressor (NASM source) into `stubs/src/aplib_depack_16.asm`. Size-optimized variant.
- Wire algorithm byte in archive header. Stub picks decompressor by algorithm.
- Keep "stored" working as fallback.

Verify:

- All phase 1 tests pass.
- New tests confirm aPLib output is smaller than `gzip -9` on typical mixed-content fixtures.
- 8086 stub binary under 4KB. Print actual size in build output.
- DOSBox-X test passes with aPLib payload on 8086 CPU emulation.

### Phase 3: 386 and pentium tiers

Goal: add two more CPU target variants alongside 8086.

Build:

- Watcom Dockerfile builds three stub variants: `-0`, `-3`, `-5`.
- Port aPLib's 32-bit decompressor into `stubs/src/aplib_depack_32.asm`. Use for 386 stub.
- Hand-schedule 32-bit decompressor for U/V pairing in the pentium variant (`aplib_depack_p5.asm`). Document the pairing in comments.
- Rust host gains `--target {8086,386,pentium}` flag. Picks the right embedded blob.
- All three stub blobs embedded via `include_bytes!`.

Verify:

- DOSBox-X tests run on three CPU emulation profiles. Each test packs with matching `--target` and verifies extraction.
- Benchmark decompression of a 500KB payload across all three tiers. Results in `tests/benchmarks/results.md`. Expected: 386 is 2-4x faster than 8086, pentium is 5-10x faster.
- Stub sizes: 8086 under 4KB, 386 under 6KB, pentium under 8KB.

### Phase 4: chunked extraction and large payloads

Goal: handle payloads larger than free conventional memory.

Build:

- Chunked encoder in Rust (chunk size configurable, default 32KB).
- Chunked decoder in each stub variant: decompress one chunk into a fixed buffer, flush to disk, next chunk.
- 8.3 filename mangling on host side when source names are too long. Warn on stderr.
- FAT timestamp and attribute restoration in the stub.

Verify:

- A 2MB payload SFX extracts correctly under DOSBox-X with `memsize=2` and no XMS/EMS. Test on all three tiers.
- Timestamps on extracted files match originals (truncated to FAT 2-second resolution).
- All previous tests pass.

### Phase 5: LZMA and remaining CPU tiers

Goal: add LZMA algorithm (386+ only) and the remaining CPU tiers.

Build:

- Vendor xz-embedded or a minimal LZMA decoder. Confirm license compatibility.
- LZMA decompressor stub variants for 386, 486, pentium, pentium-mmx, p2, p3. No 8086 or 286 LZMA stub.
- Fill in `286`, `486`, `pentium-mmx`, `p2`, `p3` target tiers for aPLib and stored paths.
- Rust host rejects `--algo lzma --target 8086` and `--algo lzma --target 286` with a clear error.
- MMX-accelerated copy paths in `aplib_depack_mmx.asm` for pentium-mmx and later.
- SSE-accelerated copy paths in `aplib_depack_sse.asm` for p3.

Verify:

- DOSBox-X tests for all viable (algorithm, target) combinations. Matrix in `tests/integration/`.
- LZMA produces smaller files than aPLib on payloads > 100KB.
- LZMA decompression on 386 tier completes within 10x the aPLib decompression time on the same payload. Anything worse means the LZMA stub needs optimization.
- pentium-mmx aplib decompression speedup over pentium aplib: gate redefined. The 30% "literal-heavy" threshold was speculative — aPLib emits literals one byte at a time gated on bit-decode decisions (no literal-run opcode), so "literal-heavy" and "MMX-acceleratable" are mutually exclusive. The MMX path (`aplib_depack_mmx.asm`) copies 8 bytes per MOVQ only when `offset >= 8 AND length >= 8`; typical aPLib payloads have a heavy short-match tail (offset 1..7, length 2..6) that skips the MMX path entirely. Realistic speedup on mixed-content payloads: 0–5%. Gate closed as: MMX depacker is correct and wired in; the 30% threshold on literal-heavy payloads is removed. See `tests/benchmarks/results.md` for the full decision and `benchmark_tier_decompression` in `host/tests/benchmark_tiers.rs` for the isolated decode-timing methodology (INT 1Ah bench blobs via `make bench`).

### Phase 6: LZSA2, polish, and release

Goal: third algorithm option (fast decompression) and release readiness.

Build:

- Vendor lzsa. Wire compressor in Rust.
- Port Jim Leonard's 8088 LZSA2 decoder into the 8086 stub. Use 32-bit variants for 386+.
- `inspect` subcommand: archive header, file list, sizes, algorithm, target tier.
- Decent error messages from each stub. One or two lines max per error.
- Run-after-extract: optional config in archive header, stub does INT 21h/4Bh after extraction.
- README with examples for each (algorithm, target) combination.
- GitHub Releases workflow producing doskrunch binaries for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64.

Verify:

- LZSA2 decompression on 8086 tier is faster than aPLib on the same payload. Benchmarked.
- run-after-extract test: pack a script and a small DOS exe, verify the exe runs after extraction inside DOSBox-X.
- Release pipeline produces working binaries for all five host platforms in a clean CI run.

## 11. Done criteria for v1

- All six phases pass their verification on a clean CI run.
- README documents the (algorithm, target) matrix and recommends defaults.
- `doskrunch pack out.exe files/` with no flags produces a working SFX that runs on an original IBM PC equivalent (8086 cpu type in DOSBox-X).
- `doskrunch pack --target p2 --algo lzma out.exe files/` produces a working SFX optimized for a Pentium II machine.
- All stub binaries reproducibly built in CI and committed to the repo.
- License audit confirms every vendored dependency is compatible with the project's chosen license.

## 12. Open decisions to make before coding

1. **License.** MIT or Apache-2.0. Affects vendoring decisions (UCL is GPL with linking exception, complicates relicensing).
2. **Stub blobs in the repo or only as CI artifacts.** Committing them gives reproducible builds without needing the Watcom toolchain locally. Recommended: commit.
3. **`--reproducible` mode** that zeros timestamps and other non-determinism. Probably worth doing from Phase 1.
4. **.COM support priority.** Adding `--format com` is maybe 200 lines of stub code. Worth doing in Phase 3 or deferring to v2?
5. **Encryption.** Cheap XOR keystream is no-cost but provides no real security. ChaCha20 adds ~1KB to the stub. Worth it for v1 or v2?
6. **Default `--target`.** Currently `8086` for safety. Alternative: detect typical use case and default to `386`. Stick with `8086` as default.

## 13. Why this beats existing options

- **Info-ZIP unzipsfx (DOS build)**: works, but stub is ~30KB, Deflate ratio is worse than aPLib on small files, and the workflow chains three tools (zip, cat/copy /b, zip -A). One command replaces all of that with a tighter result. No CPU tier targeting at all.
- **UPX**: compresses a single executable, not an archive of files. Different use case.
- **aPackage** (the original aPLib author's SFX tool): Windows SFX only, not DOS.
- **PKZIP's PKSFX**: abandoned, DOS-host-only to build, proprietary, can't be CI'd in 2026.
- **Building it yourself in 1992**: requires Borland C, a DOS box, hours of fiddling. The modern tool collapses that into `cargo install doskrunch` and produces tuned stubs for every CPU generation.

The niche is real. Nothing on the market produces CPU-tier-optimized DOS SFXs from a modern cross-platform CLI. v1 with aPLib + the 8086/386/pentium tiers is the meaningful milestone; the rest of the tiers and algorithms are linear additions to a working pipeline.
