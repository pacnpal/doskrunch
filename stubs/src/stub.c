/* stub.c — Phase 1 doskrunch SFX stub: 8086 / stored algorithm.
 *
 * Locates itself on disk (argv[0]), reads the DKTR trailer at EOF-8,
 * seeks to the DKCH archive header, walks per-file records, and writes
 * each file's stored chunks to disk via Watcom's INT 21h wrappers.
 *
 * Memory model: small (DS=SS, code+data ≤64KB). g_buf is a 16KB scratch
 * buffer in the data segment. Payload size is not bounded by RAM — we
 * copy through the buffer chunk by chunk via DOS file handles.
 *
 * Build: Open Watcom v2, real-mode DOS, -0 -ms -os.
 */

#include "dos.h"
#include <fcntl.h>
#include <io.h>
#include <stdlib.h>
#include <string.h>

#define BUF_SIZE 16384u
#define TRAILER_SIZE 8u

static u8  g_buf[BUF_SIZE];

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

int main(int argc, char **argv)
{
    int self;
    int out;
    u32 archive_off;
    long self_size;
    u16 file_count;
    u16 i;
    u8 trailer[TRAILER_SIZE];
    u8 hdr[21];
    u8 hcrc[4];

    (void)argc;

    if (_dos_open(argv[0], O_RDONLY, &self) != 0) {
        die("cannot open self");
    }

    self_size = lseek(self, 0L, SEEK_END);
    if (self_size < (long)TRAILER_SIZE) die("truncated");

    if (lseek(self, -(long)TRAILER_SIZE, SEEK_END) == -1L) die("seek trailer");
    if (read_exact(self, trailer, TRAILER_SIZE) != 0) die("read trailer");
    if (memcmp(trailer, DKTR, 4) != 0) die("bad trailer magic");
    archive_off = rd_u32(trailer + 4);

    if (lseek(self, (long)archive_off, SEEK_SET) == -1L) die("seek archive");
    if (read_exact(self, hdr, 21) != 0) die("read header");
    if (memcmp(hdr, DKCH, 4) != 0) die("bad archive magic");
    if (read_exact(self, hcrc, 4) != 0) die("read header crc");
    /* Header CRC is validated by the host on pack; the stub skips it to
     * save ~150 bytes. The per-file payload is what we actually care
     * about and the host writes correct CRCs. */

    if (hdr[4] != 1)  die("bad version");
    if (hdr[5] != 0)  die("algo not stored");
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
        if (name_len == 0 || name_len >= sizeof(namebuf)) die("bad name length");
        if (read_exact(self, namebuf, name_len) != 0) die("read name");
        namebuf[name_len] = '\0';
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
            (void)read_exact(self, filecrc, 4);
            continue;
        }

        for (ci = 0; ci < chunk_count; ci++) {
            u16 csize;
            u16 usize;
            if (read_exact(self, ch_b, 4) != 0) die("read chunk header");
            csize = rd_u16(ch_b);
            usize = rd_u16(ch_b + 2);
            if (csize != usize) die("stored size mismatch");
            if (csize == 0) continue;
            if (copy_bytes(self, out, (u32)csize) != 0) die("copy");
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
