/* stub.c — doskrunch SFX stub: 8086 / stored + aplib algorithms.
 *
 * Locates itself on disk (argv[0]), reads the DKTR trailer at EOF-8,
 * seeks to the DKCH archive header, walks per-file records, and writes
 * each file's chunks to disk via Watcom's INT 21h wrappers.
 *
 * Two algorithms are dispatched at runtime on the archive's algorithm
 * byte (DKCH header offset 5):
 *   0 — stored: chunks are copied through g_buf, csize == usize.
 *   1 — aplib:  each chunk is read into g_src, decompressed via
 *               aplib_depack into g_dst, then written out. Per-chunk
 *               uncompressed size is bounded by the host (archive.rs
 *               APLIB_CHUNK_INPUT = 16 KiB) so the worst-case compressed
 *               buffer fits in small-model DS alongside g_dst.
 *
 * Memory model: small (DS=SS, code+data ≤64KB). Scratch buffers live in
 * the data segment but not in the on-disk image (BSS). Payload size is
 * not bounded by RAM; we stream through buffers chunk by chunk.
 *
 * Build: Open Watcom v2 + NASM, real-mode DOS, -0 -ms -os, linked with
 * stubs/src/aplib_depack_16.asm.
 */

#include <dos.h>
#include <fcntl.h>
#include <io.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned char  u8;
typedef unsigned short u16;
typedef unsigned long  u32;

#define BUF_SIZE 16384u
/* Worst-case aPLib expansion on BUF_SIZE bytes: n + n/8 + 16 = 18448. */
#define APLIB_SRC_SIZE 18464u
#define TRAILER_SIZE 8u

static u8  g_buf[BUF_SIZE];
static u8  g_src[APLIB_SRC_SIZE];

/* Decompress an aPLib stream pointed to by `src` into `dst`. Returns the
 * decompressed byte count. Implemented in stubs/src/aplib_depack_16.asm
 * (ported from apultra's asm/8088/aplib_8088_small.S). Watcom small-model
 * register-based calling: first arg in SI, second in DI, return in AX. */
extern unsigned int aplib_depack(const u8 *src, u8 *dst);
/* The depacker uses SI/DI as iterators and also trashes BX/CX/DX/BP.
 * Listing SI/DI explicitly in `modify` keeps the contract here in
 * lockstep with the asm header's `Trashes:` line (input registers are
 * caller-saved under Watcom's register-based calling convention, so
 * this is documentation rather than a code-gen change). */
#pragma aux aplib_depack parm [si] [di] value [ax] modify [ax bx cx dx si di bp];

static const char DKCH[4] = { 'D', 'K', 'C', 'H' };
static const char DKTR[4] = { 'D', 'K', 'T', 'R' };

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

/* Read exactly `count` bytes into `dst`. Returns 0 on success, -1 on EOF/error. */
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

/* Pipe `count` bytes from src to dst through g_buf. */
static int copy_bytes(int src, int dst, u32 count)
{
    while (count > 0) {
        unsigned take = BUF_SIZE;
        unsigned got = 0;
        unsigned wrote = 0;
        if ((u32)take > count) take = (unsigned)count;
        if (_dos_read(src, g_buf, take, &got) != 0 || got == 0) return -1;
        if (_dos_write(dst, g_buf, got, &wrote) != 0 || wrote != got) return -1;
        count -= got;
    }
    return 0;
}

static int skip_bytes(int h, u32 count)
{
    /* lseek takes signed 32-bit; loop in <2GiB chunks. */
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

/* Case-insensitive equality against an upper-case literal. `lit` must
 * be ASCII upper-case; `s` is the name stem we're checking. */
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

/* Reject names that aren't strict 8.3 ASCII basenames, contain path
 * separators or NUL, look like DOS device names (CON/PRN/AUX/NUL,
 * COM1..9, LPT1..9), or have leading dots / trailing dot or space.
 * `s` is NUL-terminated; `slen` is the strlen, computed by caller.
 * Returns 0 on accept, nonzero on reject. */
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
        /* Reject control bytes (< 0x20), DEL (0x7F), and non-ASCII. */
        if (c < 0x20 || c == 0x7f || c >= 0x80) return 1;
        /* FAT 8.3 illegal byte set — mirrors host/src/name83.rs ILLEGAL
         * so an archive that slipped past the host validator can't
         * land an unexpected/ambiguous name on DOS. */
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
    /* If there's a dot, the rest is the extension. Length <= 3, no
     * additional dots. */
    if (i < slen) {
        unsigned ext_len = slen - i - 1;
        unsigned j;
        if (ext_len > 3) return 1;
        for (j = i + 1; j < slen; j++) {
            if (s[j] == '.') return 1;
        }
    }
    /* DOS device name check on the upper-cased stem. */
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
    u8 algo;
    u8 trailer[TRAILER_SIZE];
    u8 hdr[21];
    u8 hcrc[4];

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
    /* Validate before casting to signed long: DOS lseek is i32, and the
     * archive must sit before the trailer we just read. */
    if (archive_off > 0x7FFFFFFFUL) die("archive offset > 2 GiB");
    if ((u32)archive_off + TRAILER_SIZE > (u32)self_size) die("archive offset past EOF");

    if (lseek(self, (long)archive_off, SEEK_SET) == -1L) die("seek archive");
    if (read_exact(self, hdr, 21) != 0) die("read header");
    if (memcmp(hdr, DKCH, 4) != 0) die("bad archive magic");
    if (read_exact(self, hcrc, 4) != 0) die("read header crc");
    /* Header CRC is validated by the host on pack; the stub skips it to
     * save ~150 bytes. The per-file payload is what we actually care
     * about and the host writes correct CRCs. */

    if (hdr[4] != 1)  die("bad version");
    algo = hdr[5];
    if (algo > 1) die("bad algo");
    file_count = rd_u16(hdr + 9);

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
        /* Need room for an explicit NUL: the host writes one inside
         * name_len, but a corrupted archive might not. >= leaves room. */
        /* PLAN.md §8: name_len INCLUDES the trailing NUL, so the
         * minimum valid length is 2 (one char + NUL). */
        if (name_len < 2 || name_len >= sizeof(namebuf)) die("bad name length");
        if (read_exact(self, namebuf, name_len) != 0) die("read name");
        /* Spec says the trailing NUL is INSIDE name_len; require it
         * and treat the rest as a strict 8.3 ASCII basename. Defends
         * against a hostile archive smuggling "..\\AUTOEXEC.BAT" or
         * "CON" past the host validator. */
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

        if (_dos_creat(namebuf, attrs, &out) != 0) {
            puts2("doskrunch: cannot create ");
            puts2(namebuf);
            puts2("\r\n");
            /* Stay aligned: skip the chunks + file CRC and continue. */
            for (ci = 0; ci < chunk_count; ci++) {
                if (read_exact(self, ch_b, 4) != 0) die("skip chunk hdr");
                if (skip_bytes(self, (u32)rd_u16(ch_b)) != 0) die("skip chunk");
            }
            if (read_exact(self, filecrc, 4) != 0) die("skip filecrc");
            continue;
        }

        for (ci = 0; ci < chunk_count; ci++) {
            u16 csize;
            u16 usize;
            unsigned wrote;
            unsigned produced;
            if (read_exact(self, ch_b, 4) != 0) die("read chunk header");
            csize = rd_u16(ch_b);
            usize = rd_u16(ch_b + 2);
            if (csize == 0) {
                if (usize != 0) die("zero csize, nonzero usize");
                continue;
            }
            if (algo == 0) {
                if (csize != usize) die("stored size mismatch");
                if (copy_bytes(self, out, (u32)csize) != 0) die("copy");
            } else {
                /* aplib: read whole compressed chunk, depack, write out. */
                if (csize > APLIB_SRC_SIZE) die("aplib csize");
                if (usize > BUF_SIZE)       die("aplib usize");
                if (read_exact(self, g_src, csize) != 0) die("read aplib");
                produced = aplib_depack(g_src, g_buf);
                if (produced != usize) die("aplib size");
                if (_dos_write(out, g_buf, usize, &wrote) != 0 || wrote != usize) {
                    die("write aplib");
                }
            }
        }

        if (read_exact(self, filecrc, 4) != 0) die("read filecrc");
        if (dos_date != 0 || dos_time != 0) {
            /* Best-effort: ignore failure (some DOS versions reject 5701h). */
            (void)_dos_setftime(out, dos_date, dos_time);
        }
        _dos_close(out);

        puts2("  ");
        puts2(namebuf);
        puts2("\r\n");
    }

    _dos_close(self);
    return 0;
}
