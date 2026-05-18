# Open Watcom v2 + NASM build driver for doskrunch stub binaries.
#
# Invoked inside the doskrunch-watcom container (debian:bookworm-slim
# with Open Watcom v2 unpacked into /opt/watcom and nasm installed —
# see ./Dockerfile). Builds one stub per (algorithm, CPU tier) and drops
# the result into blobs/<algo>_<tier>.bin.
#
# Phase 5 ships eight blobs (was three in Phase 3):
#   aplib_8086.bin       wcc -0 + aplib_depack_16.asm
#   aplib_286.bin        wcc -2 + aplib_depack_16.asm   (NEW)
#   aplib_386.bin        wcc -3 + aplib_depack_32.asm
#   aplib_486.bin        wcc -4 + aplib_depack_32.asm   (NEW)
#   aplib_pentium.bin    wcc -5 + aplib_depack_p5.asm
#   aplib_pentium-mmx.bin wcc -5 + aplib_depack_p5.asm  (NEW)
#   aplib_p2.bin         wcc -6 + aplib_depack_p5.asm   (NEW)
#   aplib_p3.bin         wcc -6 + aplib_depack_p5.asm   (NEW)
#
# Each blob dispatches at runtime on the archive's algorithm byte and
# handles both `stored` (0) and `aplib` (1) chunks; the host returns
# the matching blob for both algorithms on the requested --target.
#
# Tier-to-depacker mapping rationale:
#   * 286 reuses aplib_depack_16.asm — the 16-bit decoder is `cpu 8086`
#     so 286 is a strict superset; no 286-only instructions in the
#     depacker. The win at this tier is the surrounding C code (Watcom
#     emits PUSHA/POPA/IMUL-imm under -2).
#   * 486 reuses aplib_depack_32.asm — `cpu 386`, runs on every 32-bit
#     CPU. The depacker has no BSWAP / CMPXCHG / etc. opportunities;
#     the win is again on the C side.
#   * pentium-mmx/p2/p3 reuse aplib_depack_p5.asm. MMX/SSE acceleration
#     of the aPLib depacker is deliberately deferred: aPLib literals
#     are single-byte movsb and match copies use rep movsb over ranges
#     where offset < length (legitimate overlap), so MOVQ/MOVAPS would
#     break correctness on the hot path. Same Karpathy "measure before
#     optimizing" stance Phase 3 took for the p5 port's U/V scheduling.
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
	$(OUTDIR)/aplib_p3.bin

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

# aplib_depack_sse.obj is intentionally not built today; the .asm is
# kept on disk for follow-up investigation (see the p3-tier comment
# below for the deferral note). Reintroduce this rule when the SSE
# path is wired back in.

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
$(OUTDIR)/aplib_8086.bin: $(SRC)/stub.obj $(SRC)/aplib_depack_16.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_8086.exe \
	  file $(SRC)/stub.obj file $(SRC)/aplib_depack_16.obj
	@cp $(OUTDIR)/aplib_8086.exe $(OUTDIR)/aplib_8086.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_8086.bin); \
	  echo "aplib_8086.bin: $$SZ bytes (target ≤4096, hard ceiling 8192)"; \
	  if [ $$SZ -gt 8192 ]; then echo "ERROR: 8086 stub exceeds 8KB hard ceiling"; exit 1; fi

# ----- 286 tier --------------------------------------------------------

$(SRC)/stub_286.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_286) -fo=stub_286.obj stub.c

$(OUTDIR)/aplib_286.bin: $(SRC)/stub_286.obj $(SRC)/aplib_depack_16.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_286.exe \
	  file $(SRC)/stub_286.obj file $(SRC)/aplib_depack_16.obj
	@cp $(OUTDIR)/aplib_286.exe $(OUTDIR)/aplib_286.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_286.bin); \
	  echo "aplib_286.bin: $$SZ bytes (target ≤4096, hard ceiling 8192)"; \
	  if [ $$SZ -gt 8192 ]; then echo "ERROR: 286 stub exceeds 8KB hard ceiling"; exit 1; fi

# ----- 386 tier --------------------------------------------------------

# Distinct .obj filename so the 386 and 8086 stub-C objects coexist on
# disk without one rule clobbering the other when both targets are
# built (e.g. `make all`).
$(SRC)/stub_386.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_386) -fo=stub_386.obj stub.c

$(OUTDIR)/aplib_386.bin: $(SRC)/stub_386.obj $(SRC)/aplib_depack_32.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_386.exe \
	  file $(SRC)/stub_386.obj file $(SRC)/aplib_depack_32.obj
	@cp $(OUTDIR)/aplib_386.exe $(OUTDIR)/aplib_386.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_386.bin); \
	  echo "aplib_386.bin: $$SZ bytes (target ≤6144, hard ceiling 10240)"; \
	  if [ $$SZ -gt 10240 ]; then echo "ERROR: 386 stub exceeds 10KB hard ceiling"; exit 1; fi

# ----- 486 tier --------------------------------------------------------

$(SRC)/stub_486.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_486) -fo=stub_486.obj stub.c

$(OUTDIR)/aplib_486.bin: $(SRC)/stub_486.obj $(SRC)/aplib_depack_32.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_486.exe \
	  file $(SRC)/stub_486.obj file $(SRC)/aplib_depack_32.obj
	@cp $(OUTDIR)/aplib_486.exe $(OUTDIR)/aplib_486.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_486.bin); \
	  echo "aplib_486.bin: $$SZ bytes (target ≤6144, hard ceiling 10240)"; \
	  if [ $$SZ -gt 10240 ]; then echo "ERROR: 486 stub exceeds 10KB hard ceiling"; exit 1; fi

# ----- pentium tier ----------------------------------------------------

$(SRC)/stub_p5.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P5) -fo=stub_p5.obj stub.c

$(OUTDIR)/aplib_pentium.bin: $(SRC)/stub_p5.obj $(SRC)/aplib_depack_p5.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_pentium.exe \
	  file $(SRC)/stub_p5.obj file $(SRC)/aplib_depack_p5.obj
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

$(OUTDIR)/aplib_pentium-mmx.bin: $(SRC)/stub_pmmx.obj $(SRC)/aplib_depack_mmx.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_pentium-mmx.exe \
	  file $(SRC)/stub_pmmx.obj file $(SRC)/aplib_depack_mmx.obj
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

$(OUTDIR)/aplib_p2.bin: $(SRC)/stub_p6.obj $(SRC)/aplib_depack_mmx.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_p2.exe \
	  file $(SRC)/stub_p6.obj file $(SRC)/aplib_depack_mmx.obj
	@cp $(OUTDIR)/aplib_p2.exe $(OUTDIR)/aplib_p2.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_p2.bin); \
	  echo "aplib_p2.bin: $$SZ bytes (target ≤8192, hard ceiling 12288)"; \
	  if [ $$SZ -gt 12288 ]; then echo "ERROR: p2 stub exceeds 12KB hard ceiling"; exit 1; fi

# ----- p3 tier ---------------------------------------------------------
#
# wcc -6 codegen + MMX aplib depacker. The SSE depacker variant lives
# in stubs/src/aplib_depack_sse.asm but is NOT linked in — under
# DOSBox-X cputype=pentium_iii the MOVUPS-based block copy hangs on
# multi-chunk payloads (the small-fixture gate passes, the 500 KiB
# multi-chunk gate doesn't). NASM disassembly of the loop encoding
# looks correct, so the symptom is most likely a DOSBox-X SSE
# emulation gap rather than a depacker bug; left for follow-up
# investigation. p3 still benefits from wcc -6 codegen for the
# surrounding C housekeeping and the MMX path for long matches.

$(SRC)/stub_p3.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_P6) -fo=stub_p3.obj stub.c

$(OUTDIR)/aplib_p3.bin: $(SRC)/stub_p3.obj $(SRC)/aplib_depack_mmx.obj
	@mkdir -p $(OUTDIR)
	$(WLINK) option quiet system dos \
	  name $(OUTDIR)/aplib_p3.exe \
	  file $(SRC)/stub_p3.obj file $(SRC)/aplib_depack_mmx.obj
	@cp $(OUTDIR)/aplib_p3.exe $(OUTDIR)/aplib_p3.bin
	@SZ=$$(stat -c %s $(OUTDIR)/aplib_p3.bin); \
	  echo "aplib_p3.bin: $$SZ bytes (target ≤10240, hard ceiling 14336)"; \
	  if [ $$SZ -gt 14336 ]; then echo "ERROR: p3 stub exceeds 14KB hard ceiling"; exit 1; fi

clean:
	rm -f $(OUTDIR)/*.exe $(OUTDIR)/*.bin
	rm -f $(SRC)/*.o $(SRC)/*.obj *.o *.obj *.err
