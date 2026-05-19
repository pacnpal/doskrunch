# Open Watcom v2 + NASM build driver for doskrunch stub binaries.
#
# Invoked inside the doskrunch-watcom container (debian:bookworm-slim
# with Open Watcom v2 unpacked into /opt/watcom and nasm installed —
# see ./Dockerfile). Builds one stub per (algorithm, CPU tier) and drops
# the result into blobs/<algo>_<tier>.bin.
#
# Phase 5 ships eight aplib blobs (was three in Phase 3) plus six LZMA
# blobs:
#   aplib_8086.bin        wcc -0 + aplib_depack_16.asm
#   aplib_286.bin         wcc -2 + aplib_depack_16.asm   (NEW)
#   aplib_386.bin         wcc -3 + aplib_depack_32.asm
#   aplib_486.bin         wcc -4 + aplib_depack_32.asm   (NEW)
#   aplib_pentium.bin     wcc -5 + aplib_depack_p5.asm
#   aplib_pentium-mmx.bin wcc -5 + aplib_depack_mmx.asm  (NEW, MMX MOVQ block copy)
#   aplib_p2.bin          wcc -6 + aplib_depack_mmx.asm  (NEW, MMX MOVQ block copy)
#   aplib_p3.bin          wcc -6 + aplib_depack_sse.asm  (NEW, SSE MOVUPS block copy)
#   lzma_<tier>.bin       wcc -3..-6 -mc + xz-embedded   (NEW; 386..p3, see LZMA section)
#
# Each aplib_<tier>.bin dispatches at runtime on the archive's
# algorithm byte and handles both `stored` (0) and `aplib` (1)
# chunks; the host returns the matching aplib blob for both
# algorithms on the requested --target.
#
# The lzma_<tier>.bin variants are LZMA-only — they require the
# archive's algorithm byte to be `lzma` (3) and die loudly on
# anything else. The host's stub_for() routes `--algo lzma` packs
# at 386+ to the matching lzma_<tier>.bin and never to an aplib
# blob; the runtime gate just defends against a hand-crafted
# mis-targeted archive.
#
# Tier-to-depacker mapping rationale:
#   * 286 reuses aplib_depack_16.asm — the 16-bit decoder is `cpu 8086`
#     so 286 is a strict superset; no 286-only instructions in the
#     depacker. The win at this tier is the surrounding C code (Watcom
#     emits PUSHA/POPA/IMUL-imm under -2).
#   * 486 reuses aplib_depack_32.asm — `cpu 386`, runs on every 32-bit
#     CPU. The depacker has no BSWAP / CMPXCHG / etc. opportunities;
#     the win is again on the C side.
#   * pentium-mmx/p2 link aplib_depack_mmx.asm — an MMX-accelerated
#     aPLib depacker. The MMX path uses 8-byte MOVQ block copy when
#     the back-reference offset is >= 8 AND length is >= 8. The
#     offset >= 8 floor matches the MOVQ stride, so each 8-byte load
#     reads bytes the previous 8-byte store already wrote; length >
#     offset is therefore still safe as long as offset stays at or
#     above the chunk stride. Matches with offset < 8 (the canonical
#     compression-of-zeros case) would have MOVQ pull bytes that
#     hadn't been written yet, so they stay on the scalar `rep movsb`
#     path — same path the pentium tier uses unconditionally. EMMS
#     is emitted on exit unconditionally so a future x87 user can't
#     see stale MMX tag words.
#   * p3 links aplib_depack_sse.asm — an SSE-accelerated aPLib depacker
#     with a 16-byte MOVUPS block copy path when offset >= 16 AND
#     length >= 16, scalar `rep movsb` fallback otherwise.
#
# Usage:
#   make all      # build every shipped tier
#   make clean
#
# Driven by GNU make. The Dockerfile installs make and exports
# WATCOM/PATH/INCLUDE/EDPATH so `wcc`, `wlink` and `nasm` resolve
# without further setup.

OUTDIR  := blobs
SRC     := src
WCC     := wcc
WLINK   := wlink
NASM    := nasm

# LZMA stub variants (Phase 5) compile xz-embedded straight from
# vendor/xz-embedded/ so we don't duplicate the upstream source under
# stubs/. The CI workflow + the local build invocation both mount the
# project root and run make from /work/stubs, so `../vendor/...` is
# the correct relative path. See .github/workflows/build-stubs.yml.
VENDOR_XZ := ../vendor/xz-embedded
XZ_SRC    := $(VENDOR_XZ)/linux/lib/xz
XZ_INC    := $(VENDOR_XZ)/linux/include/linux
XZ_CFG    := $(VENDOR_XZ)/userspace

# Watcom common flags: small memory model (`-ms`), max size optimization
# (`-os`), DOS target (`-bt=dos`), quiet (`-zq`). The per-tier rules add
# the CPU-architecture flag (`-0` .. `-6`).
WCC_FLAGS_COMMON := -bt=dos -ms -os -zq
WCC_FLAGS_8086   := $(WCC_FLAGS_COMMON) -0
WCC_FLAGS_286    := $(WCC_FLAGS_COMMON) -2
WCC_FLAGS_386    := $(WCC_FLAGS_COMMON) -3
WCC_FLAGS_486    := $(WCC_FLAGS_COMMON) -4
WCC_FLAGS_P5     := $(WCC_FLAGS_COMMON) -5
WCC_FLAGS_P6     := $(WCC_FLAGS_COMMON) -6

.PHONY: all clean

all: \
	$(OUTDIR)/aplib_8086.bin \
	$(OUTDIR)/aplib_286.bin \
	$(OUTDIR)/aplib_386.bin \
	$(OUTDIR)/aplib_486.bin \
	$(OUTDIR)/aplib_pentium.bin \
	$(OUTDIR)/aplib_pentium-mmx.bin \
	$(OUTDIR)/aplib_p2.bin \
	$(OUTDIR)/aplib_p3.bin \
	$(OUTDIR)/lzma_386.bin \
	$(OUTDIR)/lzma_486.bin \
	$(OUTDIR)/lzma_pentium.bin \
	$(OUTDIR)/lzma_pentium-mmx.bin \
	$(OUTDIR)/lzma_p2.bin \
	$(OUTDIR)/lzma_p3.bin

# ----- shared NASM .obj rules ------------------------------------------
# Each depacker .asm is `cpu 8086|386|pentium` and consumed by every
# tier in its family; the .obj is shared across rules.

$(SRC)/aplib_depack_16.obj: $(SRC)/aplib_depack_16.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/aplib_depack_32.obj: $(SRC)/aplib_depack_32.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/aplib_depack_p5.obj: $(SRC)/aplib_depack_p5.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/aplib_depack_mmx.obj: $(SRC)/aplib_depack_mmx.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/aplib_depack_sse.obj: $(SRC)/aplib_depack_sse.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/lzsa2_depack_16.obj: $(SRC)/lzsa2_depack_16.asm
	$(NASM) -f obj -o $@ $<

$(SRC)/lzsa2_depack_32.obj: $(SRC)/lzsa2_depack_32.asm
	$(NASM) -f obj -o $@ $<

# exec_dos.asm — cpu 8086, linked into every tier blob (both aplib and
# lzma families).  The same .obj is reused across all tiers because the
# wrapper is pure 8086 and has no tier-specific code.
$(SRC)/exec_dos.obj: $(SRC)/exec_dos.asm
	$(NASM) -f obj -o $@ $<
# ----- 8086 tier -------------------------------------------------------

# Compile in $(SRC) so wcc deposits the .obj next to the source.
# `-fo=stub.obj` is REQUIRED — Open Watcom v2 wcc does not default
# to "<basename>.obj in CWD"; without -fo it silently produces no
# output and still exits 0, which broke the build twice during Phase 1.
$(SRC)/stub.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_8086) -fo=stub.obj stub.c

# Direct wlink invocation. wlink reads its directive language from
# the command line: `system dos` picks the DOS-mz output format and
# pulls in the small-model C runtime; `name` sets the output file;
# each `file` keyword names an input object.
$(OUTDIR)/aplib_8086.bin: $(SRC)/stub.obj $(SRC)/aplib_depack_16.obj $(SRC)/lzsa2_depack_16.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_8086.exe \
	  file $(SRC)/stub.obj \
	  file $(SRC)/aplib_depack_16.obj \
	  file $(SRC)/lzsa2_depack_16.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_8086.exe $(OUTDIR)/aplib_8086.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_8086.bin); \
	  echo "aplib_8086.bin: $$SZ bytes (target ≤4096, hard ceiling 8192)"; \
	  if [ $$SZ -gt 8192 ]; then echo "ERROR: 8086 stub exceeds 8KB hard ceiling"; exit 1; fi

# ----- 286 tier --------------------------------------------------------

$(SRC)/stub_286.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_286) -fo=stub_286.obj stub.c

$(OUTDIR)/aplib_286.bin: $(SRC)/stub_286.obj $(SRC)/aplib_depack_16.obj $(SRC)/lzsa2_depack_16.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_286.exe \
	  file $(SRC)/stub_286.obj \
	  file $(SRC)/aplib_depack_16.obj \
	  file $(SRC)/lzsa2_depack_16.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_286.exe $(OUTDIR)/aplib_286.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_286.bin); \
	  echo "aplib_286.bin: $$SZ bytes (target ≤4096, hard ceiling 8192)"; \
	  if [ $$SZ -gt 8192 ]; then echo "ERROR: 286 stub exceeds 8KB hard ceiling"; exit 1; fi

# ----- 386 tier --------------------------------------------------------

# Distinct .obj filename so the 386 and 8086 stub-C objects coexist on
# disk without one rule clobbering the other when both targets are
# built (e.g. `make all`).
$(SRC)/stub_386.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_386) -DDK_BENCH_TIMING -fo=stub_386.obj stub.c

$(OUTDIR)/aplib_386.bin: $(SRC)/stub_386.obj $(SRC)/aplib_depack_32.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_386.exe \
	  file $(SRC)/stub_386.obj \
	  file $(SRC)/aplib_depack_32.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_386.exe $(OUTDIR)/aplib_386.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_386.bin); \
	  echo "aplib_386.bin: $$SZ bytes (target ≤6144, hard ceiling 10240)"; \
	  if [ $$SZ -gt 10240 ]; then echo "ERROR: 386 stub exceeds 10KB hard ceiling"; exit 1; fi

# ----- 486 tier --------------------------------------------------------

$(SRC)/stub_486.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_486) -DDK_BENCH_TIMING -fo=stub_486.obj stub.c

$(OUTDIR)/aplib_486.bin: $(SRC)/stub_486.obj $(SRC)/aplib_depack_32.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_486.exe \
	  file $(SRC)/stub_486.obj \
	  file $(SRC)/aplib_depack_32.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_486.exe $(OUTDIR)/aplib_486.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_486.bin); \
	  echo "aplib_486.bin: $$SZ bytes (target ≤6144, hard ceiling 10240)"; \
	  if [ $$SZ -gt 10240 ]; then echo "ERROR: 486 stub exceeds 10KB hard ceiling"; exit 1; fi

# ----- pentium tier ----------------------------------------------------

$(SRC)/stub_p5.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P5) -DDK_BENCH_TIMING -fo=stub_p5.obj stub.c

$(OUTDIR)/aplib_pentium.bin: $(SRC)/stub_p5.obj $(SRC)/aplib_depack_p5.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_pentium.exe \
	  file $(SRC)/stub_p5.obj \
	  file $(SRC)/aplib_depack_p5.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_pentium.exe $(OUTDIR)/aplib_pentium.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_pentium.bin); \
	  echo "aplib_pentium.bin: $$SZ bytes (target ≤8192, hard ceiling 12288)"; \
	  if [ $$SZ -gt 12288 ]; then echo "ERROR: pentium stub exceeds 12KB hard ceiling"; exit 1; fi

# ----- pentium-mmx tier ------------------------------------------------
#
# wcc -5 codegen + MMX-accelerated aplib depacker (aplib_depack_mmx.asm).
# The MMX path copies 8 bytes per MOVQ when the back-reference offset
# is >= 8 (no overlap) and length is >= 8; otherwise falls back to the
# same scalar rep movsb the pentium tier uses. EMMS-on-exit is emitted
# unconditionally so a future x87-using caller can't see stale MMX tag
# words.

$(SRC)/stub_pmmx.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P5) -fo=stub_pmmx.obj stub.c

$(OUTDIR)/aplib_pentium-mmx.bin: $(SRC)/stub_pmmx.obj $(SRC)/aplib_depack_mmx.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_pentium-mmx.exe \
	  file $(SRC)/stub_pmmx.obj \
	  file $(SRC)/aplib_depack_mmx.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_pentium-mmx.exe $(OUTDIR)/aplib_pentium-mmx.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_pentium-mmx.bin); \
	  echo "aplib_pentium-mmx.bin: $$SZ bytes (target ≤8192, hard ceiling 12288)"; \
	  if [ $$SZ -gt 12288 ]; then echo "ERROR: pentium-mmx stub exceeds 12KB hard ceiling"; exit 1; fi

# ----- p2 tier ---------------------------------------------------------
#
# Pentium Pro / P6 codegen for the C housekeeping (wcc -6: CMOV plus
# whatever P6 OoO scheduling Watcom emits) and the MMX depacker
# (P6 has MMX baseline). Same depacker as pentium-mmx.

$(SRC)/stub_p6.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P6) -fo=stub_p6.obj stub.c

$(OUTDIR)/aplib_p2.bin: $(SRC)/stub_p6.obj $(SRC)/aplib_depack_mmx.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_p2.exe \
	  file $(SRC)/stub_p6.obj \
	  file $(SRC)/aplib_depack_mmx.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_p2.exe $(OUTDIR)/aplib_p2.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_p2.bin); \
	  echo "aplib_p2.bin: $$SZ bytes (target ≤8192, hard ceiling 12288)"; \
	  if [ $$SZ -gt 12288 ]; then echo "ERROR: p2 stub exceeds 12KB hard ceiling"; exit 1; fi

# ----- p3 tier ---------------------------------------------------------
#
# wcc -6 codegen + SSE aplib depacker. The SSE path copies 16 bytes per
# MOVUPS when match offset >= 16 AND length >= 16. The offset >= 16
# floor matches the MOVUPS stride, so each 16-byte load reads bytes
# the previous 16-byte store already wrote; length > offset is still
# safe as long as offset stays at or above the chunk stride. Matches
# with offset < 16 stay on scalar `rep movsb`, which copies byte-at-
# a-time and correctly handles the classical LZ77 run-overlap case.

$(SRC)/stub_p3.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P6) -fo=stub_p3.obj stub.c

$(OUTDIR)/aplib_p3.bin: $(SRC)/stub_p3.obj $(SRC)/aplib_depack_sse.obj $(SRC)/lzsa2_depack_32.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_p3.exe \
	  file $(SRC)/stub_p3.obj \
	  file $(SRC)/aplib_depack_sse.obj \
	  file $(SRC)/lzsa2_depack_32.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/aplib_p3.exe $(OUTDIR)/aplib_p3.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_p3.bin); \
	  echo "aplib_p3.bin: $$SZ bytes (target ≤10240, hard ceiling 14336)"; \
	  if [ $$SZ -gt 14336 ]; then echo "ERROR: p3 stub exceeds 14KB hard ceiling"; exit 1; fi

# ----- LZMA tier blobs (Phase 5) ---------------------------------------
#
# Distinct from the aplib blobs because the LZMA decoder + workspace
# blow past the aplib stub's small-model BSS budget. The host's
# stub_for() dispatches by (algorithm, target) so a `--algo lzma`
# pack at any 386+ tier picks the matching lzma_<tier>.bin; the
# aplib stub remains the dispatch target for stored + aplib.
#
# Per-tier source files:
#   stub_lzma.c          stubs/src/stub_lzma.c — LZMA-only stub.c
#   xz_crc32.c           vendor/xz-embedded — CRC32 lookup table init
#   xz_dec_lzma2.c       vendor/xz-embedded — LZMA2 + MicroLZMA decoder
#
# Each tier compiles all three under its own wcc flag so the .obj
# files don't collide across tiers. We pass -DXZ_DEC_MICROLZMA so
# xz-embedded exposes the MicroLZMA APIs that stub_lzma.c calls.
#
# All LZMA stubs need wcc -3 or higher: the LZMA decoder uses 32-bit
# integer arithmetic that 8086 / 286 can't run in reasonable time
# (PLAN.md §4 calls this out explicitly).

# LZMA stubs use the COMPACT memory model (-mc): code is near (single
# code segment), data is far (multiple data segments, far pointers
# everywhere). The aplib stubs use small model (-ms, code+data ≤ 64 KB
# each). The LZMA decoder state alone is bigger than small-model
# malloc's 32 KB per-allocation cap (`struct xz_dec_microlzma` ≈ 32 KB
# with default lc/lp/pb tables), so it has to live in a data segment
# distinct from the rest of the stub. xz-embedded's bare `uint8_t *`
# pointers become `uint8_t __far *` automatically under -mc, so we
# don't have to patch the vendor source.
XZ_CFLAGS_COMMON := -bt=dos -mc -os -zq -DXZ_DEC_MICROLZMA -i=$(XZ_INC) -i=$(XZ_SRC) -i=$(XZ_CFG)
XZ_CFLAGS_386    := $(XZ_CFLAGS_COMMON) -3
XZ_CFLAGS_486    := $(XZ_CFLAGS_COMMON) -4
XZ_CFLAGS_P5     := $(XZ_CFLAGS_COMMON) -5
XZ_CFLAGS_P6     := $(XZ_CFLAGS_COMMON) -6

# stub_lzma.c is compact-model too so it can interop with xz-embedded.
# Includes xz.h on the path.
LZMA_STUB_CFLAGS_COMMON := -bt=dos -mc -os -zq -i=$(XZ_INC) -i=$(XZ_CFG)

# 386 tier ---
# Build from the Makefile dir (no cd) so the include paths in
# LZMA_STUB_CFLAGS_COMMON resolve from there. The aplib stub rules
# still cd into $(SRC) because they don't need any include path
# resolution beyond Watcom's defaults.
$(SRC)/stub_lzma_386.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -3 -DDK_BENCH_TIMING -fo=$@ $<

$(SRC)/xz_crc32_386.obj: $(XZ_SRC)/xz_crc32.c
	$(WCC) $(XZ_CFLAGS_386) -fo=$@ $<

$(SRC)/xz_dec_lzma2_386.obj: $(XZ_SRC)/xz_dec_lzma2.c
	$(WCC) $(XZ_CFLAGS_386) -fo=$@ $<

$(OUTDIR)/lzma_386.bin: $(SRC)/stub_lzma_386.obj $(SRC)/xz_crc32_386.obj $(SRC)/xz_dec_lzma2_386.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_386.exe \
	  file $(SRC)/stub_lzma_386.obj \
	  file $(SRC)/xz_crc32_386.obj \
	  file $(SRC)/xz_dec_lzma2_386.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_386.exe $(OUTDIR)/lzma_386.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_386.bin); \
	  echo "lzma_386.bin: $$SZ bytes (target ≤18432, hard ceiling 24576)"; \
	  if [ $$SZ -gt 24576 ]; then echo "ERROR: lzma_386 stub exceeds 24KB hard ceiling"; exit 1; fi

# 486 tier ---
$(SRC)/stub_lzma_486.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -4 -DDK_BENCH_TIMING -fo=$@ $<

$(SRC)/xz_crc32_486.obj: $(XZ_SRC)/xz_crc32.c
	$(WCC) $(XZ_CFLAGS_486) -fo=$@ $<

$(SRC)/xz_dec_lzma2_486.obj: $(XZ_SRC)/xz_dec_lzma2.c
	$(WCC) $(XZ_CFLAGS_486) -fo=$@ $<

$(OUTDIR)/lzma_486.bin: $(SRC)/stub_lzma_486.obj $(SRC)/xz_crc32_486.obj $(SRC)/xz_dec_lzma2_486.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_486.exe \
	  file $(SRC)/stub_lzma_486.obj \
	  file $(SRC)/xz_crc32_486.obj \
	  file $(SRC)/xz_dec_lzma2_486.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_486.exe $(OUTDIR)/lzma_486.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_486.bin); \
	  echo "lzma_486.bin: $$SZ bytes (target ≤18432, hard ceiling 24576)"; \
	  if [ $$SZ -gt 24576 ]; then echo "ERROR: lzma_486 stub exceeds 24KB hard ceiling"; exit 1; fi

# pentium tier ---
$(SRC)/stub_lzma_p5.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -5 -DDK_BENCH_TIMING -fo=$@ $<

$(SRC)/xz_crc32_p5.obj: $(XZ_SRC)/xz_crc32.c
	$(WCC) $(XZ_CFLAGS_P5) -fo=$@ $<

$(SRC)/xz_dec_lzma2_p5.obj: $(XZ_SRC)/xz_dec_lzma2.c
	$(WCC) $(XZ_CFLAGS_P5) -fo=$@ $<

$(OUTDIR)/lzma_pentium.bin: $(SRC)/stub_lzma_p5.obj $(SRC)/xz_crc32_p5.obj $(SRC)/xz_dec_lzma2_p5.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_pentium.exe \
	  file $(SRC)/stub_lzma_p5.obj \
	  file $(SRC)/xz_crc32_p5.obj \
	  file $(SRC)/xz_dec_lzma2_p5.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_pentium.exe $(OUTDIR)/lzma_pentium.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_pentium.bin); \
	  echo "lzma_pentium.bin: $$SZ bytes (target ≤18432, hard ceiling 24576)"; \
	  if [ $$SZ -gt 24576 ]; then echo "ERROR: lzma_pentium stub exceeds 24KB hard ceiling"; exit 1; fi

# pentium-mmx tier --- (same wcc -5 codegen as pentium; distinct blob)
$(SRC)/stub_lzma_pmmx.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -5 -fo=$@ $<

$(OUTDIR)/lzma_pentium-mmx.bin: $(SRC)/stub_lzma_pmmx.obj $(SRC)/xz_crc32_p5.obj $(SRC)/xz_dec_lzma2_p5.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_pentium-mmx.exe \
	  file $(SRC)/stub_lzma_pmmx.obj \
	  file $(SRC)/xz_crc32_p5.obj \
	  file $(SRC)/xz_dec_lzma2_p5.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_pentium-mmx.exe $(OUTDIR)/lzma_pentium-mmx.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_pentium-mmx.bin); \
	  echo "lzma_pentium-mmx.bin: $$SZ bytes (target ≤18432, hard ceiling 24576)"; \
	  if [ $$SZ -gt 24576 ]; then echo "ERROR: lzma_pentium-mmx stub exceeds 24KB hard ceiling"; exit 1; fi

# p2 tier --- (wcc -6 / P6 codegen)
$(SRC)/stub_lzma_p6.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -6 -fo=$@ $<

$(SRC)/xz_crc32_p6.obj: $(XZ_SRC)/xz_crc32.c
	$(WCC) $(XZ_CFLAGS_P6) -fo=$@ $<

$(SRC)/xz_dec_lzma2_p6.obj: $(XZ_SRC)/xz_dec_lzma2.c
	$(WCC) $(XZ_CFLAGS_P6) -fo=$@ $<

$(OUTDIR)/lzma_p2.bin: $(SRC)/stub_lzma_p6.obj $(SRC)/xz_crc32_p6.obj $(SRC)/xz_dec_lzma2_p6.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_p2.exe \
	  file $(SRC)/stub_lzma_p6.obj \
	  file $(SRC)/xz_crc32_p6.obj \
	  file $(SRC)/xz_dec_lzma2_p6.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_p2.exe $(OUTDIR)/lzma_p2.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_p2.bin); \
	  echo "lzma_p2.bin: $$SZ bytes (target ≤18432, hard ceiling 24576)"; \
	  if [ $$SZ -gt 24576 ]; then echo "ERROR: lzma_p2 stub exceeds 24KB hard ceiling"; exit 1; fi

# p3 tier --- (same wcc -6 codegen; distinct blob like the aplib side)
$(SRC)/stub_lzma_p3.obj: $(SRC)/stub_lzma.c
	$(WCC) $(LZMA_STUB_CFLAGS_COMMON) -6 -fo=$@ $<

$(OUTDIR)/lzma_p3.bin: $(SRC)/stub_lzma_p3.obj $(SRC)/xz_crc32_p6.obj $(SRC)/xz_dec_lzma2_p6.obj $(SRC)/exec_dos.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/lzma_p3.exe \
	  file $(SRC)/stub_lzma_p3.obj \
	  file $(SRC)/xz_crc32_p6.obj \
	  file $(SRC)/xz_dec_lzma2_p6.obj \
	  file $(SRC)/exec_dos.obj
	@cp $(OUTDIR)/lzma_p3.exe $(OUTDIR)/lzma_p3.bin
	@SZ=$$(stat -c %s $(OUTDIR)/lzma_p3.bin); \
	  echo "lzma_p3.bin: $$SZ bytes (target ≤20480, hard ceiling 28672)"; \
	  if [ $$SZ -gt 28672 ]; then echo "ERROR: lzma_p3 stub exceeds 28KB hard ceiling"; exit 1; fi

clean:
	rm -f $(OUTDIR)/*.exe $(OUTDIR)/*.bin
	rm -f $(SRC)/*.o $(SRC)/*.obj *.o *.obj *.err
