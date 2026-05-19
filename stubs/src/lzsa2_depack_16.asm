;  lzsa2_depack_16.asm - 16-bit LZSA2 depacker for the doskrunch
;  8086 / 286 stubs.
;
;  Source lineage:
;    Ported byte-for-byte from lzsa's `asm/8088/decompress_small_v2.S`
;    (Emmanuel Marty's 8088 small-size decoder, with help from Jim
;    Leonard / Trixter on the speed-tuned variant). Upstream copy
;    lives in this tree at
;      vendor/lzsa/asm/8088/decompress_small_v2.S
;    and decodes the raw LZSA2 block stream emitted by the host
;    (`host/src/compress/lzsa2.rs` with LZSA_FLAG_RAW_BLOCK).
;
;  License: Zlib (vendor/lzsa/LICENSE.zlib.md). MIT-compatible.
;
;  Adaptations for doskrunch:
;    * `_TEXT` segment with `public class=CODE use16` so Open Watcom
;      wlink links this OMF object alongside Watcom-compiled C in a
;      small-model executable. Upstream uses `.text` (GNU as / NASM
;      flat-bin convention) which wlink doesn't recognize.
;    * Public symbol renamed `lzsa2_decompress` -> `lzsa2_depack` to
;      match the aplib_depack naming convention. C-side `#pragma aux
;      ... "*"` keeps the verbatim symbol (no underscore mangling),
;      same trick the aplib depacker uses.
;    * Save/restore ES at entry/exit. The upstream decoder writes
;      through ES:DI but doesn't establish ES itself; this wrapper
;      sets `ES = DS` (small-model data segment) on entry so the
;      C-side `#pragma aux` declaration only has to pass DI in the
;      data segment.
;    * Save/restore BP at entry/exit. Watcom's small-model `-os`
;      keeps a frame pointer in BP and reads locals as `[bp-N]` after
;      we return; without this push/pop the surrounding function's
;      frame dies (same fix Phase 2 made for the aplib 8086 port).
;    * Internal labels prefixed with `l2_` (no NASM `.local` scoping
;      under `-f obj`; the upstream `.decode_token` etc. names would
;      collide if any other module ever uses the same `.local` names
;      in the same translation-unit-set).

        cpu     8086
        bits    16

        segment _TEXT public class=CODE use16
        global  lzsa2_depack

;  ---------------------------------------------------------------------------
;  Decompress raw LZSA2 block.
;
;  C-side declaration (small model, register-based calling — canonical
;  pragma lives in stubs/src/stub.c, repeated here for reference):
;    extern unsigned int lzsa2_depack(const u8 *src, u8 *dst);
;    #pragma aux lzsa2_depack "*" parm [si] [di] value [ax] \
;                                  modify exact [ax bx cx dx si di];
;
;  inputs:
;    ds:si  raw LZSA2 block
;    es:di  destination buffer (this wrapper sets es=ds on entry)
;
;  output:
;    ax     decompressed byte count
;
;  Trashes: ax, bx, cx, dx, si, di, bp, flags. es is preserved.
;  ---------------------------------------------------------------------------

lzsa2_depack:
        push    es                      ; preserve caller's es
        push    bp                      ; preserve Watcom's frame pointer
        push    ds
        pop     es                      ; es = ds (small-model data segment)

        push    di                      ; remember decompression offset
        cld                             ; lods/movs/stos move forward

        xor     cx, cx
        mov     bx, 0100h
        xor     bp, bp

l2_decode_token:
        mov     ax, cx                  ; clear ah (cx is zero from above or after rep movsb)
        lodsb                           ; read token byte XYZ|LL|MMMM
        mov     dx, ax                  ; keep token in dl

        and     al, 018h                ; isolate literals length (LL)
        mov     cl, 3
        shr     al, cl                  ; shift literals length into place

        cmp     al, 03h                 ; LITERALS_RUN_LEN_V2?
        jne     l2_got_literals

        call    l2_get_nibble           ; extra literals length nibble
        add     al, cl
        cmp     al, 012h                ; LITERALS_RUN_LEN_V2 + 15?
        jne     l2_got_literals

        lodsb                           ; extra length byte
        add     al, 012h                ; overflow?
        jnc     l2_got_literals

        lodsw                           ; 16-bit extra length

l2_got_literals:
        xchg    cx, ax
        rep     movsb                   ; copy cx literals ds:si -> es:di

        test    dl, 0C0h                ; check match-offset mode in token (X bit)
        js      l2_rep_match_or_large_offset

        xchg    cx, ax                  ; clear ah (cx is zero from the rep movsb above)
        jne     l2_offset_9_bit

                                        ; 5-bit offset
        cmp     dl, 020h                ; test bit 5
        call    l2_get_nibble_x
        jmp     short l2_dec_offset_top

l2_offset_9_bit:                        ; 9-bit offset
        lodsb                           ; 8-bit offset from stream into AL
        dec     ah                      ; set offset bits 15-8 to 1
        test    dl, 020h                ; test bit Z (offset bit 8)
        je      l2_get_match_length
l2_dec_offset_top:
        dec     ah                      ; clear bit 8 if Z bit clear, else set offset bits 15-8
        jmp     short l2_get_match_length

l2_rep_match_or_large_offset:
        jpe     l2_rep_match_or_16_bit

                                        ; 13-bit offset
        cmp     dl, 0A0h                ; test bit 5 (knowing bit 7 is set)
        xchg    ah, al
        call    l2_get_nibble_x
        sub     al, 2                   ; subtract 512
        jmp     short l2_get_match_length_1

l2_rep_match_or_16_bit:
        test    dl, 020h                ; test bit Z (offset bit 8)
        jne     l2_repeat_match         ; rep-match

                                        ; 16-bit offset
        lodsb                           ; 2-byte match offset

l2_get_match_length_1:
        xchg    ah, al
        lodsb                           ; load match offset bits 0-7

l2_get_match_length:
        xchg    bp, ax                  ; bp = offset
l2_repeat_match:
        xchg    ax, dx                  ; ax = original token
        and     al, 07h                 ; isolate match length (MMM)
        add     al, 2                   ; + MIN_MATCH_SIZE_V2

        cmp     al, 09h                 ; MIN_MATCH_SIZE_V2 + MATCH_RUN_LEN_V2?
        jne     l2_got_matchlen

        call    l2_get_nibble           ; extra match length nibble
        add     al, cl
        cmp     al, 018h                ; MIN_MATCH_SIZE_V2 + MATCH_RUN_LEN_V2 + 15?
        jne     l2_got_matchlen

        lodsb                           ; extra length byte
        add     al, 018h                ; overflow?
        jnc     l2_got_matchlen
        je      short l2_done           ; EOD marker

        lodsw                           ; 16-bit extra length

l2_got_matchlen:
        xchg    cx, ax                  ; cx = match length
        push    ds                      ; save ds:si (compressed-data pointer)
        xchg    si, ax
        push    es
        pop     ds
        lea     si, [bp+di]             ; ds:si = back-reference in output data
        rep     movsb                   ; copy match
        xchg    si, ax                  ; restore ds:si
        pop     ds
        jmp     l2_decode_token

l2_done:
        pop     ax                      ; original di
        xchg    di, ax                  ; ax = current di, di = original
        sub     ax, di                  ; decompressed byte count

        pop     bp                      ; restore caller's bp
        pop     es                      ; restore caller's es
        ret

l2_get_nibble_x:
        cmc                             ; carry set if bit 4 was set
        rcr     al, 1
        call    l2_get_nibble           ; nibble for offset bits 0-3
        or      al, cl                  ; merge
        rol     al, 1
        xor     al, 0E1h                ; set offset bits 7-5 to 1
        ret

l2_get_nibble:
        neg     bh                      ; nibble ready?
        jns     l2_has_nibble

        xchg    bx, ax
        lodsb                           ; load two nibbles
        xchg    bx, ax

l2_has_nibble:
        mov     cl, 4                   ; swap 4 high / low bits of nibble
        ror     bl, cl
        mov     cl, 0Fh
        and     cl, bl
        ret
