# Open Watcom v2 + NASM build driver for doskrunch stub binaries.
#
# Invoked inside the doskrunch-watcom container (debian:bookworm-slim
# with Open Watcom v2 unpacked into /opt/watcom and nasm installed —
# see ./Dockerfile). Builds one stub per (algorithm, CPU tier) and drops
# the result into blobs/<algo>_<tier>.bin.
#
# Phase 3 ships three blobs: aplib_8086.bin (wcc -0 + aplib_depack_16.asm),
# aplib_386.bin (wcc -3 + aplib_depack_32.asm), and aplib_pentium.bin
# (wcc -5 + aplib_depack_p5.asm). Each dispatches at runtime on the
# archive's algorithm byte and handles both `stored` (0) and `aplib`
# (1) chunks; the host returns the matching blob for both algorithms
# on the requested --target tier.
#
# All three asm depackers export the same `aplib_depack` symbol with
# the same C-side ABI (small-model regparm: src in SI, dst in DI,
# return in AX), so stub.c is single-source — the Makefile picks the
# .obj per tier and the wcc CPU flag (`-0`, `-3`, `-5`) determines the
# instructions Watcom emits for the surrounding housekeeping code.
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
# the CPU-architecture flag (`-0`, `-3`, `-5`).
WCC_FLAGS_COMMON := -bt=dos -ms -os -zq
WCC_FLAGS_8086   := $(WCC_FLAGS_COMMON) -0
WCC_FLAGS_386    := $(WCC_FLAGS_COMMON) -3
WCC_FLAGS_P5     := $(WCC_FLAGS_COMMON) -5

.PHONY: all clean

all: $(OUTDIR)/aplib_8086.bin $(OUTDIR)/aplib_386.bin $(OUTDIR)/aplib_pentium.bin

# ----- 8086 tier -------------------------------------------------------

$(SRC)/aplib_depack_16.obj: $(SRC)/aplib_depack_16.asm
	$(NASM) -f obj -o $@ $<

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

# ----- 386 tier --------------------------------------------------------

$(SRC)/aplib_depack_32.obj: $(SRC)/aplib_depack_32.asm
	$(NASM) -f obj -o $@ $<

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

# ----- pentium tier ----------------------------------------------------

$(SRC)/aplib_depack_p5.obj: $(SRC)/aplib_depack_p5.asm
	$(NASM) -f obj -o $@ $<

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

clean:
	rm -f $(OUTDIR)/*.exe $(OUTDIR)/*.bin
	rm -f $(SRC)/*.o $(SRC)/*.obj *.o *.obj *.err
