# Open Watcom v2 + NASM build driver for doskrunch stub binaries.
#
# Invoked inside the doskrunch-watcom container (debian:bookworm-slim
# with Open Watcom v2 unpacked into /opt/watcom and nasm installed —
# see ./Dockerfile). Builds one stub per (algorithm, CPU tier) and drops
# the result into blobs/<algo>_<tier>.bin.
#
# Phase 2 ships one blob: `aplib_8086.bin`. It dispatches at runtime on
# the archive's algorithm byte and handles both `stored` (0) and `aplib`
# (1) chunks. The host returns the same blob for both algorithms on
# `--target 8086`.
#
# Usage:
#   make all      # build every shipped variant
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

# 8086, small model, max size optimization (-os), DOS target, real mode.
# -zq quiets the banner, -bt=dos picks the DOS 16-bit target, -ms forces
# small memory model. NOTE: `-bt=` is the wcc/wpp target flag; `-bcl=`
# is the wcl driver flag (different binary, different option syntax).
WCC_FLAGS_8086 := -bt=dos -0 -ms -os -zq

.PHONY: all clean

all: $(OUTDIR)/aplib_8086.bin

$(SRC)/aplib_depack_16.obj: $(SRC)/aplib_depack_16.asm
	$(NASM) -f obj -o $@ $<

# Compile in $(SRC) so wcc deposits `stub.obj` next to the source. wcc
# without `-fo=` writes <basename>.obj in CWD, which is the cleanest
# path on Open Watcom v2.
$(SRC)/stub.obj: $(SRC)/stub.c
	cd $(SRC) && $(WCC) $(WCC_FLAGS_8086) stub.c

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

clean:
	rm -f $(OUTDIR)/*.exe $(OUTDIR)/*.bin
	rm -f $(SRC)/*.o $(SRC)/*.obj *.o *.obj *.err
