/* stub_lzma.c — doskrunch LZMA SFX stub: 386+ tiers.
 *
 * Companion to stubs/src/stub.c (the stored+aplib stub). Same archive
 * walk + INT 21h housekeeping; the only meaningful difference is the
 * per-chunk decode path uses xz-embedded's MicroLZMA decoder instead
 * of aplib_depack.
 *
 * Algorithm gate: this stub handles ONLY `algo == 3` (LZMA) archives;
 * anything else dies loudly. The host's `stub_for` returns the LZMA
 * blob only for `--algo lzma` packs, so the runtime gate just defends
 * against a manually-constructed mis-targeted archive.
 *
 * Memory model: compact (-mc). Code is near (single code segment ≤ 64
 * KB) and data is far (multiple data segments, far pointers everywhere).
 * Picked over small (-ms) because `struct xz_dec_microlzma` alone is
 * larger than small-model malloc's 32 KB per-allocation cap with the
 * default lc/lp/pb tables, so the decoder state needs to live in a
 * data segment distinct from the stub's own BSS. Under -mc, bare
 * `uint8_t *` in vendor/xz-embedded/ becomes `uint8_t __far *`
 * automatically and `kmalloc` (→ `malloc`) returns a far pointer out
 * of the far heap, so we don't have to patch the vendor source or wrap
 * the xz_dec_microlzma_* entry points.
 *
 *   g_lzma_src (compressed in, this stub's BSS, far)  LZMA_MAX_COMPRESSED_CHUNK (~17 KiB)
 *   g_lzma_buf (uncompressed out / SINGLE-mode dict)  16 KiB
 *   xz_dec_microlzma state      kmalloc'd, separate data segment
 *   stub C runtime + stack      a few KiB in DGROUP
 *
 * The static BSS pair (`g_lzma_src` + `g_lzma_buf` ≈ 33 KiB) lives in
 * its own far data segment under -mc; the decoder state and the
 * Watcom stack land in separate segments, so we no longer have to fit
 * everything in a single 64 KiB DS.
 *
 * If a future change pushes a single allocation past 64 KiB, the next
 * step is either to cut LZMA_CHUNK_INPUT (so g_lzma_src/g_lzma_buf
 * shrink) or wrap xz_dec_microlzma_* with a huge-pointer ($DS:0)
 * helper. Moving to large (-ml) is the other lever but pulls in the
 * far-call C runtime, which costs stub bytes.
 *
 * Build: Open Watcom v2 + cc-rs'd xz-embedded C, real-mode DOS, -3 -mc
 * -os, linked with vendor/xz-embedded/{xz_crc32,xz_dec_lzma2}.obj.
 */

#include <dos.h>
#include <fcntl.h>
#include <io.h>
#include <stdlib.h>
#include <string.h>

#include "xz.h"

typedef unsigned char  u8;
typedef unsigned short u16;
typedef unsigned long  u32;

/* MUST match host/src/archive.rs::LZMA_CHUNK_INPUT (16 KiB) and
 * LZMA_MAX_COMPRESSED_CHUNK (17 KiB). LZMA_DICT_SIZE (16 KiB) is
 * implicit in XZ_SINGLE: the output buffer IS the dictionary. */
#define LZMA_BUF_SIZE     16384u
#define LZMA_SRC_SIZE     (17u * 1024u)
#define LZMA_DICT_SIZE_   16384u   /* underscore to avoid colliding with archive.rs name */
#define TRAILER_SIZE      8u

/* g_lzma_src precedes g_lzma_buf in BSS (defense-in-depth ordering;
 * an overrun from g_lzma_buf lands in zero-init BSS past DS instead
 * of corrupting g_lzma_src that the decoder is still reading from). */
static u8 g_lzma_src[LZMA_SRC_SIZE];
static u8 g_lzma_buf[LZMA_BUF_SIZE];

/* Run-after-extract command buffer (v1.1).
 * Matches host/src/archive.rs::RUN_AFTER_MAX_LEN and stub.c constant.
 * Read from archive at archive_off+run_after_offset after extraction. */
#define RUN_AFTER_BUF 128u
static char g_run_after[RUN_AFTER_BUF];

/* EXEC (INT 21h/4Bh) parameter block — same layout as stub.c.
 * Compact model: g_exec_pb is in DGROUP (same DS); FP_OFF extracts
 * the near offset correctly from both near and far pointers. */
static u16  g_exec_pb[7];

/* Counted DOS command line for the EXEC parameter block. */
static char g_exec_cmdline[130];

/* Archive header flag bits. Must match host/src/archive.rs::flags. */
#define FLAG_RUN_AFTER     0x0001u
#define FLAG_REPRODUCIBLE  0x0004u

static const char DKCH[4] = { 'D', 'K', 'C', 'H' };
static const char DKTR[4] = { 'D', 'K', 'T', 'R' };

/* INT 21h/4Bh EXEC primitive — implemented in stubs/src/exec_dos.asm.
 * Same interface as stub.c: prog_si and pb_di are near DS offsets.
 * In compact model (-mc) the pointers are far, but the static arrays
 * live in DGROUP so FP_OFF() extracts the correct near offset. */
extern unsigned exec_dos(u16 prog_si, u16 pb_di);
#pragma aux exec_dos "*" parm [si] [di] value [ax] modify exact [ax bx cx dx si di];

static u16 get_ds(void);
#pragma aux get_ds = "mov ax, ds" value [ax] modify exact [ax];

static u16 get_psp_seg(void);
#pragma aux get_psp_seg = "mov ah, 0x51" "int 0x21" value [bx] modify exact [ax bx];

static void puts2(const char *s)
{
    unsigned wrote;
    if (s) _dos_write(1, s, (unsigned)strlen(s), &wrote);
}

static void die(const char *msg)
{
    puts2("doskrunch: ");
    puts2(msg);
    puts2("\r\n");
    exit(1);
}

static int read_exact(int h, void *dst, unsigned count)
{
    unsigned char *p = (unsigned char *)dst;
    while (count > 0) {
        unsigned got = 0;
        if (_dos_read(h, p, count, &got) != 0) return -1;
        if (got == 0) return -1;
        p += got;
        count -= got;
    }
    return 0;
}

static int skip_bytes(int h, u32 count)
{
    while (count > 0) {
        long step = count > 0x40000000UL ? 0x40000000L : (long)count;
        if (lseek(h, step, SEEK_CUR) == -1L) return -1;
        count -= (u32)step;
    }
    return 0;
}

static u16 rd_u16(const u8 *p) { return (u16)p[0] | ((u16)p[1] << 8); }
static u32 rd_u32(const u8 *p)
{
    return (u32)p[0]
         | ((u32)p[1] << 8)
         | ((u32)p[2] << 16)
         | ((u32)p[3] << 24);
}

#ifdef DKRUNCH_BENCH_TICKS
/* Bench-only decode timing — see the same block in stubs/src/stub.c. The
 * LZMA decoder gets its own copy of these helpers because stub_lzma.c is a
 * separate program. Compiled ONLY into the lzma_*_bench.bin blobs
 * (`make bench`), never into the shipped `make all` blobs: the shipped
 * stub must not carry the repeat-decode loop or the DKPERF.BIN sidecar. */
static u32 bios_ticks_now(void)
{
    union REGS inregs, outregs;
    inregs.h.ah = 0x00;
    int86(0x1a, &inregs, &outregs);
    return ((u32)outregs.x.cx << 16) | (u32)outregs.x.dx;
}

/* INT 1Ah wraps at MIDNIGHT (0x1800B0 ticks), not 2^32; a plain t1-t0 is
 * wrong across a rollover. Sub-second decode makes this defensive, but
 * compute the wrap-correct delta anyway. Mirrors stub.c::tick_delta. */
#define BIOS_TICKS_PER_DAY 0x1800B0UL
static u32 tick_delta(u32 t0, u32 t1)
{
    return (t1 >= t0) ? (t1 - t0) : (u32)(BIOS_TICKS_PER_DAY - t0 + t1);
}

/* BENCH_PA.BIN (8.3 form of bench_payload.bin) gates the perf sidecar so a
 * normal extraction doesn't repeat-decode or write DKPERF.BIN. */
static int is_bench_payload_name(const char *s)
{
    static const char BENCH[] = "BENCH_PA";
    unsigned i = 0;
    while (s[i] != '\0' && s[i] != '.' && i < sizeof(BENCH) - 1) {
        char c = s[i];
        if (c >= 'a' && c <= 'z') c = (char)(c - 32);
        if (c != BENCH[i]) return 0;
        i++;
    }
    return i == sizeof(BENCH) - 1;
}
#endif /* DKRUNCH_BENCH_TICKS */

static int ieq_upper(const char *s, unsigned slen, const char *lit)
{
    unsigned i;
    for (i = 0; i < slen; i++) {
        char c = s[i];
        if (c >= 'a' && c <= 'z') c = (char)(c - 32);
        if (lit[i] == '\0' || c != lit[i]) return 0;
    }
    return lit[slen] == '\0';
}

/* Same 8.3 ASCII validation as stubs/src/stub.c. Kept inline rather
 * than moved to a shared header so each stub variant remains self-
 * contained (Watcom's small-model linker doesn't multi-include from
 * a shared header well, and the validation is only ~50 lines). */
static int validate_name(const char *s, unsigned slen)
{
    unsigned i;
    unsigned stem_len = 0;
    static const char *RESERVED[] = {
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
        "LPT6", "LPT7", "LPT8", "LPT9",
    };
    if (slen == 0 || slen > 12) return 1;
    if (s[0] == '.') return 1;
    if (s[slen - 1] == '.' || s[slen - 1] == ' ') return 1;
    for (i = 0; i < slen; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c < 0x20 || c == 0x7f || c >= 0x80) return 1;
        if (c == ' '  || c == '"' || c == '*' || c == '+' || c == ','
         || c == '/'  || c == ':' || c == ';' || c == '<' || c == '='
         || c == '>'  || c == '?' || c == '[' || c == '\\' || c == ']'
         || c == '|') return 1;
        if (c == '.') {
            stem_len = i;
            break;
        }
    }
    if (i == slen) stem_len = slen;
    if (stem_len == 0 || stem_len > 8) return 1;
    if (i < slen) {
        unsigned ext_len = slen - i - 1;
        unsigned j;
        if (ext_len > 3) return 1;
        for (j = i + 1; j < slen; j++) {
            if (s[j] == '.') return 1;
        }
    }
    for (i = 0; i < sizeof(RESERVED) / sizeof(RESERVED[0]); i++) {
        if (ieq_upper(s, stem_len, RESERVED[i])) return 1;
    }
    return 0;
}

int main(int argc, char **argv)
{
    int self;
    int out;
    u32 archive_off;
    long self_size;
    u16 file_count;
    u16 i;
    u16 flags;
    u16 run_after_offset;
    u8 algo;
#ifdef DKRUNCH_BENCH_TICKS
    int perf_mode = 0;
    u32 lzma_ticks_total = 0;
#endif
    u8 trailer[TRAILER_SIZE];
    u8 hdr[21];
    u8 hcrc[4];
    struct xz_dec_microlzma *dec;

    (void)argc;

    if (_dos_open(argv[0], O_RDONLY, &self) != 0) {
        die("cannot open self");
    }

    self_size = lseek(self, 0L, SEEK_END);
    if (self_size == -1L) die("seek end");
    if (self_size < (long)TRAILER_SIZE) die("truncated");

    if (lseek(self, -(long)TRAILER_SIZE, SEEK_END) == -1L) die("seek trailer");
    if (read_exact(self, trailer, TRAILER_SIZE) != 0) die("read trailer");
    if (memcmp(trailer, DKTR, 4) != 0) die("bad trailer magic");
    archive_off = rd_u32(trailer + 4);
    if (archive_off > 0x7FFFFFFFUL) die("archive offset > 2 GiB");
    if ((u32)archive_off + TRAILER_SIZE > (u32)self_size) die("archive offset past EOF");

    if (lseek(self, (long)archive_off, SEEK_SET) == -1L) die("seek archive");
    if (read_exact(self, hdr, 21) != 0) die("read header");
    if (memcmp(hdr, DKCH, 4) != 0) die("bad archive magic");
    /* hcrc is read but intentionally not verified. Matches stubs/src/
     * stub.c (the aplib/stored stub) so the two stubs have identical
     * archive-walk contracts; verifying CRCs across all stub variants
     * is tracked as a follow-up "unify integrity checking" change
     * rather than landing asymmetrically in the LZMA stub. Reading
     * the bytes keeps the stream position correct for the per-file
     * loop below. */
    if (read_exact(self, hcrc, 4) != 0) die("read header crc");
    (void)hcrc; /* silence any future -Wunused-variable */

    if (hdr[4] != 1)  die("bad version");
    algo = hdr[5];
    /* LZMA-only stub: refuse any other algorithm. The host's stub_for
     * dispatches by (algo, target), so seeing algo != 3 here means an
     * out-of-tree producer mis-targeted the archive. */
    if (algo != 3) die("lzma stub: not an lzma archive");
    flags = rd_u16(hdr + 7);
    file_count = rd_u16(hdr + 9);
    /* Header layout (must match host/src/archive.rs): bytes 11-14
     * total_uncompressed, 15-18 total_compressed, 19-20
     * run_after_offset. Mirrors the same fix applied to stub.c. */
    run_after_offset = rd_u16(hdr + 19);

    xz_crc32_init();
    /* XZ_SINGLE: the output buffer IS the dictionary; no separate
     * dict allocation. Per the xz-embedded contract this returns NULL
     * only on malloc failure or invalid dict_size. The dict_size
     * argument bounds what we'll accept from the stream — anything
     * the encoder produced with LZMA_DICT_SIZE on the host side is
     * within this. */
    dec = xz_dec_microlzma_alloc(XZ_SINGLE, LZMA_DICT_SIZE_);
    if (!dec) die("lzma alloc");

    for (i = 0; i < file_count; i++) {
        u8  name_len_b;
        u8  attrs;
        u8  ts_b[4];
        u8  usz_b[4];
        u8  cc_b[2];
        u8  ch_b[4];
        u8  filecrc[4];
        u16 chunk_count;
        u16 ci;
        u16 dos_date;
        u16 dos_time;
        u32 ts;
        char namebuf[14];
        unsigned name_len;

        if (read_exact(self, &name_len_b, 1) != 0) die("read name len");
        name_len = name_len_b;
        if (name_len < 2 || name_len >= sizeof(namebuf)) die("bad name length");
        if (read_exact(self, namebuf, name_len) != 0) die("read name");
        if (namebuf[name_len - 1] != '\0') die("name not nul-terminated");
        namebuf[sizeof(namebuf) - 1] = '\0';
        if (validate_name(namebuf, (unsigned)(name_len - 1)) != 0) {
            die("unsafe name");
        }
        if (read_exact(self, &attrs, 1) != 0) die("read attrs");
        if (read_exact(self, ts_b, 4) != 0)   die("read ts");
        ts = rd_u32(ts_b);
        dos_time = (u16)(ts & 0xFFFFu);
        dos_date = (u16)((ts >> 16) & 0xFFFFu);

        if (read_exact(self, usz_b, 4) != 0)  die("read usz");
        if (read_exact(self, cc_b, 2) != 0)   die("read cc");
        chunk_count = rd_u16(cc_b);

        if (_dos_creat(namebuf, attrs & 0x27, &out) != 0) {
            puts2("doskrunch: cannot create ");
            puts2(namebuf);
            puts2("\r\n");
            for (ci = 0; ci < chunk_count; ci++) {
                if (read_exact(self, ch_b, 4) != 0) die("skip chunk hdr");
                if (skip_bytes(self, (u32)rd_u16(ch_b)) != 0) die("skip chunk");
            }
            if (read_exact(self, filecrc, 4) != 0) die("skip filecrc");
            continue;
        }

#ifdef DKRUNCH_BENCH_TICKS
        /* Enable perf timing only after the output file was created (a failed
         * _dos_creat above would have `continue`d), so DKPERF.BIN ticks
         * always reflect a real extraction. */
        if (is_bench_payload_name(namebuf)) perf_mode = 1;
#endif

        for (ci = 0; ci < chunk_count; ci++) {
            u16 csize;
            u16 usize;
            unsigned wrote;
            struct xz_buf b;
            enum xz_ret ret;

            if (read_exact(self, ch_b, 4) != 0) die("read chunk header");
            csize = rd_u16(ch_b);
            usize = rd_u16(ch_b + 2);
            if (csize == 0) {
                if (usize != 0) die("zero csize, nonzero usize");
                continue;
            }
            if (csize > LZMA_SRC_SIZE) die("lzma csize");
            if (usize > LZMA_BUF_SIZE) die("lzma usize");
            if (read_exact(self, g_lzma_src, csize) != 0) die("read lzma");

            /* Reset for this chunk. uncomp_size_is_exact=1 because the
             * DKCH header per-chunk usize is authoritative. The reset +
             * xz_buf setup must repeat for each timed iteration so the
             * decoder starts clean every pass. */
#ifdef DKRUNCH_BENCH_TICKS
            {
                /* One tick pair around all reps (reset + run), not per rep:
                 * at ~55 ms/tick a single decode floors to whole ticks, so
                 * bracketing the whole repeated block gives a stable read and
                 * avoids per-rep INT 1Ah overhead. The reset stays inside the
                 * loop so each pass starts the decoder clean. */
                u16 rep;
                u16 reps = (u16)((perf_mode != 0) ? 8u : 1u);
                u32 t0 = bios_ticks_now();
                for (rep = 0; rep < reps; rep++) {
                    xz_dec_microlzma_reset(dec, csize, usize, 1);
                    b.in = g_lzma_src;
                    b.in_pos = 0;
                    b.in_size = csize;
                    b.out = g_lzma_buf;
                    b.out_pos = 0;
                    b.out_size = usize;
                    ret = xz_dec_microlzma_run(dec, &b);
                    if (ret != XZ_STREAM_END) die("lzma decode");
                    if (b.out_pos != usize) die("lzma size");
                }
                lzma_ticks_total += tick_delta(t0, bios_ticks_now());
            }
#else
            xz_dec_microlzma_reset(dec, csize, usize, 1);

            b.in = g_lzma_src;
            b.in_pos = 0;
            b.in_size = csize;
            b.out = g_lzma_buf;
            b.out_pos = 0;
            b.out_size = usize;

            ret = xz_dec_microlzma_run(dec, &b);
            if (ret != XZ_STREAM_END) die("lzma decode");
            if (b.out_pos != usize) die("lzma size");
#endif

            if (_dos_write(out, g_lzma_buf, usize, &wrote) != 0 || wrote != usize) {
                die("write lzma");
            }
        }

        if (read_exact(self, filecrc, 4) != 0) die("read filecrc");
        if (dos_date != 0 || dos_time != 0) {
            (void)_dos_setftime(out, dos_date, dos_time);
        }
        _dos_close(out);

        puts2("  ");
        puts2(namebuf);
        puts2("\r\n");
    }

    xz_dec_microlzma_end(dec);

#ifdef DKRUNCH_BENCH_TICKS
    /* Bench-only: write the total LZMA decode ticks to DKPERF.BIN as a
     * little-endian u32 so the host harness can read it after DOSBox-X
     * exits. perf_mode is only set when the BENCH_PA.BIN payload is
     * present, so a normal extraction never creates this file. */
    if (perf_mode) {
        int perf_h;
        if (_dos_creat("DKPERF.BIN", 0x20, &perf_h) != 0) die("create DKPERF");
        {
            unsigned wrote = 0;
            /* Fail loudly on a short/failed write so the host harness gets a
             * clear dosbox-nonzero failure instead of "missing DKPERF.BIN". */
            if (_dos_write(perf_h, &lzma_ticks_total, 4, &wrote) != 0 || wrote != 4u) {
                die("write DKPERF");
            }
            _dos_close(perf_h);
        }
    }
#endif /* DKRUNCH_BENCH_TICKS */

    /* Run-after-extract (v1.1): INT 21h/4Bh EXEC. Same logic as
     * stub.c; compact-model pointer differences are absorbed by
     * FP_OFF() which works on both near and far pointers. */
    if (flags & FLAG_RUN_AFTER) {
        unsigned got = 0;
        char *space;
        char *args;
        unsigned args_len;
        u16 ds_val;
        u16 psp;

        if (lseek(self, (long)archive_off + (long)run_after_offset, SEEK_SET) != -1L
         && _dos_read(self, g_run_after, RUN_AFTER_BUF, &got) == 0
         && got > 0u) {

            g_run_after[RUN_AFTER_BUF - 1] = '\0';

            space = (char *)memchr(g_run_after, ' ', RUN_AFTER_BUF);
            if (space != 0) {
                *space = '\0';
                args = space + 1;
            } else {
                args = g_run_after + (unsigned)strlen(g_run_after);
            }
            args_len = (unsigned)strlen(args);
            if (args_len > 126u) args_len = 126u;

            if (args_len > 0u) {
                g_exec_cmdline[0] = (char)(args_len + 1u);
                g_exec_cmdline[1] = ' ';
                memcpy(&g_exec_cmdline[2], args, args_len);
                g_exec_cmdline[(unsigned)args_len + 2u] = '\r';
            } else {
                g_exec_cmdline[0] = '\0';
                g_exec_cmdline[1] = '\r';
            }

            ds_val = get_ds();
            psp    = get_psp_seg();
            g_exec_pb[0] = 0u;
            g_exec_pb[1] = (u16)FP_OFF(g_exec_cmdline);
            g_exec_pb[2] = ds_val;
            g_exec_pb[3] = 0x005Cu;
            g_exec_pb[4] = psp;
            g_exec_pb[5] = 0x006Cu;
            g_exec_pb[6] = psp;

            _dos_close(self);
            exec_dos((u16)FP_OFF(g_run_after), (u16)FP_OFF(g_exec_pb));
            return 0;
        }
    }

    _dos_close(self);
    return 0;
}
