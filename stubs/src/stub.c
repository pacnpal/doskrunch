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

/* IMPORTANT: must match host/src/archive.rs::APLIB_CHUNK_INPUT (16 KiB).
 * If you bump one, bump the other and verify build_aplib_entry's
 * compressed-chunk ceiling assertion still holds. */
#define BUF_SIZE 16384u
/* Worst-case aPLib expansion on BUF_SIZE bytes: n + n/8 + 16 = 18448.
 * Producer-side ceiling is enforced by archive.rs::APLIB_MAX_COMPRESSED_CHUNK
 * which must stay <= APLIB_SRC_SIZE so a host-produced archive can never
 * trip the runtime `aplib csize` die() on a real DOS box. */
#define APLIB_SRC_SIZE 18464u
#define TRAILER_SIZE 8u

/* `g_src` precedes `g_buf` in BSS so a (hypothetical) aplib decompressor
 * over-run from `g_buf` lands in zero-init BSS past the end of the data
 * segment instead of corrupting the live compressed-input buffer that
 * the depacker is still reading from. Defense-in-depth on top of the
 * trust-boundary documented near `aplib_depack`'s extern decl. */
static u8  g_src[APLIB_SRC_SIZE];
static u8  g_buf[BUF_SIZE];

/* Run-after-extract command buffer (v1.1).
 * Matches host/src/archive.rs::RUN_AFTER_MAX_LEN.
 * Read from the archive at archive_off+run_after_offset after extraction
 * and then split into prog-name (DS:DX for INT 21h/4Bh) and args. */
#define RUN_AFTER_BUF 128u
static char g_run_after[RUN_AFTER_BUF];

/* EXEC (INT 21h/4Bh) parameter block. 14 bytes, filled at run-after time.
 * Layout (Ralf Brown's Interrupt List, function 4Bh type 0):
 *   +00 u16  env_seg:     0 = inherit parent environment
 *   +02 u16  cmdline_off: near offset (in DS) of counted command line
 *   +04 u16  cmdline_seg: segment of counted command line = DS
 *   +06 u16  fcb1_off:    near offset of default FCB1 in parent PSP (0x5C)
 *   +08 u16  fcb1_seg:    segment of FCB1 = PSP segment
 *   +0A u16  fcb2_off:    near offset of default FCB2 in parent PSP (0x6C)
 *   +0C u16  fcb2_seg:    segment of FCB2 = PSP segment */
static u16  g_exec_pb[7];

/* Counted DOS command line for the EXEC parameter block.
 * Format: 1-byte length (not counting the CR) + optional " "+args + CR.
 * Worst case: 1 (len) + 1 (space) + 126 (max args) + 1 (CR) = 129 bytes. */
static char g_exec_cmdline[130];

/* Archive header flag bits. Must match host/src/archive.rs::flags. */
#define FLAG_RUN_AFTER     0x0001u
#define FLAG_REPRODUCIBLE  0x0004u

/* Decompress an aPLib stream pointed to by `src` into `dst`. Returns the
 * decompressed byte count. Implemented in stubs/src/aplib_depack_16.asm
 * (ported from apultra's asm/8088/aplib_8088_small.S).
 *
 * Watcom small-model register-based calling: first arg in SI, second in
 * DI, return in AX. The `"*"` name override suppresses Watcom's default
 * trailing-underscore mangling so the symbol matches the NASM-emitted
 * `aplib_depack` (no underscore). Without it, wlink errors with
 * `undefined symbol aplib_depack_`.
 *
 * `modify exact` lists everything the asm actually trashes EXCEPT
 * registers the wrapper preserves (BP, ES). The wrapper restores BP
 * because Watcom keeps a `[bp-N]` frame pointer in BP — if we claimed
 * BP as caller-clobbered, Watcom would also emit useless save/restore
 * for it around the call.
 *
 * TRUST BOUNDARY: this depacker has no destination-capacity argument
 * and stops only when the bitstream itself emits the EOD marker. A
 * corrupted/hostile archive could declare `usize <= BUF_SIZE` while
 * the bitstream decompresses to more bytes, walking DI past the end
 * of g_buf into g_src and the rest of BSS before control returns to
 * the `produced != usize` check below. The host enforces the chunk
 * input ceiling (archive.rs APLIB_CHUNK_INPUT = 16 KiB) and rewrites
 * a fresh per-file CRC32 over the uncompressed bytes during pack;
 * end-to-end integrity is checked by the host on unpack but the stub
 * deliberately skips that check (~150 bytes of code) and relies on
 * the upstream depacker not running away on well-formed apultra
 * output. If you're feeding archives from an untrusted source, host-
 * unpack first — the stub's threat model assumes the producer is
 * trusted. */
extern unsigned int aplib_depack(const u8 *src, u8 *dst);
#pragma aux aplib_depack "*" parm [si] [di] value [ax] modify exact [ax bx cx dx si di];

/* INT 21h/4Bh EXEC primitive. prog_si and pb_di are near offsets within
 * DS of the program path and EXEC parameter block, respectively.
 * Implemented in stubs/src/exec_dos.asm (cpu 8086, linked into every tier).
 * Returns 0 if the child ran and exited; nonzero on EXEC load failure.
 * SS:SP, DS, ES, and BP are all preserved across this call. */
extern unsigned exec_dos(u16 prog_si, u16 pb_di);
#pragma aux exec_dos "*" parm [si] [di] value [ax] modify exact [ax bx cx dx si di];

/* Inline helper: return the current DS register value. Used to fill the
 * segment fields in g_exec_pb (cmdline_seg) at run-after time. */
static u16 get_ds(void);
#pragma aux get_ds = "mov ax, ds" value [ax] modify exact [ax];

/* Inline helper: return our PSP segment via INT 21h/51h (DOS 2.0+).
 * PSP:0x5C and PSP:0x6C are used as the default FCB1/FCB2 pointers in
 * the EXEC parameter block, matching the DOS convention. */
static u16 get_psp_seg(void);
#pragma aux get_psp_seg = "mov ah, 0x51" "int 0x21" value [bx] modify exact [ax bx];

/* LZSA2 raw-block decompressor (Phase 6). Same Watcom small-model
 * regparm ABI as aplib_depack — src in SI, dst in DI, return in AX.
 * Implemented in stubs/src/lzsa2_depack_{16,32}.asm (the Makefile
 * picks the .obj per tier). Trust boundary: same caveats as
 * aplib_depack apply — the depacker has no destination-capacity
 * argument and stops at the LZSA2 EOD marker, so a corrupted block
 * could walk DI past the end of g_buf. The host enforces
 * LZSA2_CHUNK_INPUT = 16 KiB and rewrites a per-file CRC32 during
 * pack; same trusted-producer threat model documented above. */
extern unsigned int lzsa2_depack(const u8 *src, u8 *dst);
#pragma aux lzsa2_depack "*" parm [si] [di] value [ax] modify exact [ax bx cx dx si di];

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
    u16 flags;
    u16 run_after_offset;
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
    /* This stub handles algo 0 (stored), 1 (aplib), and 2 (lzsa2).
     * LZMA (algo == 3) is in a separate per-tier blob (stub_lzma.c)
     * because its decoder state + dict don't fit in our small-model
     * BSS. The host's stub_for() routes by (algo, target), so seeing
     * algo == 3 here means a stub-vs-archive mismatch. */
    if (algo > 2) die("bad algo");
    flags = rd_u16(hdr + 7);
    file_count = rd_u16(hdr + 9);
    /* Header layout (must match host/src/archive.rs): bytes 11-14 are
     * total_uncompressed, 15-18 are total_compressed, 19-20 are the
     * run_after_offset u16. The previous revision read offset 15 by
     * mistake, which would have picked up the low half of
     * total_compressed when the v1.1 stub starts honoring this. */
    run_after_offset = rd_u16(hdr + 19);

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

        /* Strip directory (0x10) and volume-label (0x08) bits — the host
         * always writes 0x20 (archive) so an archive with those bits set
         * came from a hostile or buggy producer. Keep only the safe
         * file-attribute subset: archive | system | hidden | read-only. */
        if (_dos_creat(namebuf, attrs & 0x27, &out) != 0) {
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
            } else if (algo == 1) {
                /* aplib: read whole compressed chunk, depack, write out. */
                if (csize > APLIB_SRC_SIZE) die("aplib csize");
                if (usize > BUF_SIZE)       die("aplib usize");
                if (read_exact(self, g_src, csize) != 0) die("read aplib");
                produced = aplib_depack(g_src, g_buf);
                if (produced != usize) die("aplib size");
                if (_dos_write(out, g_buf, usize, &wrote) != 0 || wrote != usize) {
                    die("write aplib");
                }
            } else {
                /* lzsa2 (algo == 2). Same shape as aplib, different
                 * depacker. Per-chunk compressed cap matches
                 * archive.rs::LZSA2_MAX_COMPRESSED_CHUNK = 17 KiB and
                 * fits in g_src (APLIB_SRC_SIZE = 18464 B); the
                 * uncompressed cap is LZSA2_CHUNK_INPUT = 16 KiB,
                 * exactly BUF_SIZE. Buffers are shared with the aplib
                 * path so the stub doesn't pay extra BSS for LZSA2. */
                if (csize > APLIB_SRC_SIZE) die("lzsa2 csize");
                if (usize > BUF_SIZE)       die("lzsa2 usize");
                if (read_exact(self, g_src, csize) != 0) die("read lzsa2");
                produced = lzsa2_depack(g_src, g_buf);
                if (produced != usize) die("lzsa2 size");
                if (_dos_write(out, g_buf, usize, &wrote) != 0 || wrote != usize) {
                    die("write lzsa2");
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

    /* Run-after-extract (v1.1): INT 21h/4Bh EXEC. */
    if (flags & FLAG_RUN_AFTER) {
        unsigned got = 0;
        char *space;
        char *args;
        unsigned args_len;
        u16 ds_val;
        u16 psp;

        /* Seek to and read the command string. run_after_offset is the
         * byte offset of the NUL-terminated command relative to the
         * start of the DKCH archive header (archive_off). Skip the
         * whole run-after block on any I/O error; the files are already
         * extracted so the SFX exit remains clean. */
        if (lseek(self, (long)archive_off + (long)run_after_offset, SEEK_SET) != -1L
         && _dos_read(self, g_run_after, RUN_AFTER_BUF, &got) == 0
         && got > 0u) {

            /* Paranoia NUL-guard: the host always writes a NUL-
             * terminated string, but defend against a truncated read. */
            g_run_after[RUN_AFTER_BUF - 1] = '\0';

            /* Split at first space: left half = program name (NUL-
             * terminated in place), right half = args (may be empty). */
            space = (char *)memchr(g_run_after, ' ', RUN_AFTER_BUF);
            if (space != 0) {
                *space = '\0';
                args = space + 1;
            } else {
                args = g_run_after + (unsigned)strlen(g_run_after);
            }
            args_len = (unsigned)strlen(args);
            if (args_len > 126u) args_len = 126u;

            /* Build the counted command line for the EXEC param block:
             *   byte 0: character count (not including the CR)
             *   bytes 1..n: optional ' ' + args
             *   last byte: CR (0x0D) terminator */
            if (args_len > 0u) {
                g_exec_cmdline[0] = (char)(args_len + 1u); /* +1 for leading space */
                g_exec_cmdline[1] = ' ';
                memcpy(&g_exec_cmdline[2], args, args_len);
                g_exec_cmdline[(unsigned)args_len + 2u] = '\r';
            } else {
                g_exec_cmdline[0] = '\0'; /* zero-length tail */
                g_exec_cmdline[1] = '\r';
            }

            /* Fill the EXEC parameter block. DS = DGROUP in small
             * model, so near offsets are sufficient for far pointers.
             * FP_OFF() extracts the near offset from any pointer;
             * in small model this is just the pointer value. */
            ds_val = get_ds();
            psp    = get_psp_seg();
            g_exec_pb[0] = 0u;                           /* env_seg: inherit */
            g_exec_pb[1] = (u16)FP_OFF(g_exec_cmdline);  /* cmdline offset */
            g_exec_pb[2] = ds_val;                        /* cmdline segment = DS */
            g_exec_pb[3] = 0x005Cu;                       /* FCB1 offset (PSP:5Ch) */
            g_exec_pb[4] = psp;                           /* FCB1 segment = PSP */
            g_exec_pb[5] = 0x006Cu;                       /* FCB2 offset (PSP:6Ch) */
            g_exec_pb[6] = psp;                           /* FCB2 segment = PSP */

            /* Close self before exec so the child doesn't inherit our
             * SFX file handle. exec_dos() returns when the child exits
             * (or immediately on load failure). */
            _dos_close(self);
            exec_dos((u16)FP_OFF(g_run_after), (u16)FP_OFF(g_exec_pb));
            /* exec_dos only returns on EXEC load failure; fall through. */
            return 0;
        }
    }

    _dos_close(self);
    return 0;
}
