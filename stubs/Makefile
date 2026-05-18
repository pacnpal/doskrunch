# Open Watcom v2 build driver for doskrunch stub binaries.
#
# Invoked inside the volkertb/debian-open-watcom container (Linux host),
# so commands use POSIX tools. Builds one stub per (algorithm, CPU tier)
# and drops the result into blobs/<algo>_<tier>.bin.
#
# Phase 1 ships only (stored, 8086). Phase 2+ adds aplib_8086. Phase 3
# adds 386 and pentium tiers.
#
# Usage:
#   make all      # build every shipped variant
#   make clean
#
# Driven by GNU make; the volkertb/debian-open-watcom image already has
# `make` available. Watcom tools are on PATH via /etc/profile.d/.

OUTDIR  := blobs
SRC     := src
WCL     := wcl

# 8086, small model, max size optimization (-os), DOS target, real mode.
# -zq quiets the banner, -bcl=dos picks the DOS 16-bit target,
# -ms forces small memory model.
CFLAGS_8086 := -bcl=dos -0 -ms -os -zq -I$(SRC)

.PHONY: all clean

all: $(OUTDIR)/stored_8086.bin

$(OUTDIR)/stored_8086.bin: $(SRC)/stub.c
	@mkdir -p $(OUTDIR)
	$(WCL) $(CFLAGS_8086) -fe=$(OUTDIR)/stored_8086.exe $(SRC)/stub.c
	@cp $(OUTDIR)/stored_8086.exe $(OUTDIR)/stored_8086.bin
	@SZ=$$(stat -c %s $(OUTDIR)/stored_8086.bin); \
	  echo "stored_8086.bin: $$SZ bytes"; \
	  if [ $$SZ -gt 8192 ]; then echo "ERROR: 8086 stub exceeds 8KB hard ceiling"; exit 1; fi

clean:
	rm -f $(OUTDIR)/*.exe $(OUTDIR)/*.bin
	rm -f $(SRC)/*.o $(SRC)/*.obj *.o *.obj *.err
