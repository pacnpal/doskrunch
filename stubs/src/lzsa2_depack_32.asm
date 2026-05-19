;  lzsa2_depack_32.asm - 32-bit-register LZSA2 depacker for the
;  doskrunch 386 / 486 / pentium / pentium-mmx / p2 / p3 stubs
;  (bits 16, cpu 386). Linked into every aplib_<tier>.bin from 386
;  onward; the same blob handles stored / aplib / lzsa2 via runtime
;  dispatch on the archive's algo byte. Exports `lzsa2_depack` with
;  the same C-side regparm ABI as `aplib_depack`.
;
;  Source lineage:
;    Ported from lzsa's `asm/x86/decompress_small_v2.asm`, Emmanuel
;    Marty's 32-bit size-optimized decoder. Upstream copy lives at
;      vendor/lzsa/asm/x86/decompress_small_v2.asm
;    and round-trips the raw LZSA2 block stream emitted by the host
;    (`host/src/compress/lzsa2.rs` with LZSA_FLAG_RAW_BLOCK).
;
;  License: Zlib.
;
;  Adaptations for doskrunch (mirrors `aplib_depack_32.asm`):
;    * `bits 16 / cpu 386`. Target is 16-bit real-mode DOS on a 386+
;      CPU. NASM auto-emits 0x66 (operand-size) and 0x67 (address-
;      size) prefixes wherever 32-bit registers appear under bits 16.
;    * `_TEXT` segment `public class=CODE use16` so Open Watcom wlink
;      links this OMF object alongside Watcom-compiled C in a small-
;      model executable.
;    * Public symbol `lzsa2_depack` (no underscore; Watcom register-
;      based calling, declared on the C side via `#pragma aux ... "*"`).
;    * Watcom small-model regparm ABI: first arg in SI, second in DI,
;      return in AX. Upstream uses `[esp+32+4]` / `[esp+32+8]` to fish
;      the args off the stack after `pushad`. We don't pushad — we
;      take args in SI / DI directly, widen with movzx, and let the C-
;      side `modify exact` clause cover the trashed registers.
;    * Save/restore ES at entry/exit. Watcom small-model pins
;      CS=DS=SS but not ES; the depacker writes through ES:[EDI] so
;      this wrapper sets ES=DS on entry.
;    * Save/restore BP at entry/exit. Watcom small-model `-os` keeps
;      a frame pointer in BP and reads locals as `[bp-N]` after we
;      return; without this push/pop the surrounding function's
;      frame dies.
;    * Internal labels prefixed with `l2_` (no NASM `.local` scoping
;      under `-f obj`).
;    * Return value: AX = decompressed byte count (16-bit; chunks are
;      bounded by archive.rs::LZSA2_CHUNK_INPUT = 16 KiB so the count
;      always fits in AX).

        cpu     386
        bits    16

        segment _TEXT public class=CODE use16
        global  lzsa2_depack

lzsa2_depack:
        push    es                      ; preserve caller's es
        push    bp                      ; preserve Watcom's frame pointer
        push    ds
        pop     es                      ; es = ds (small-model data segment)

        push    di                      ; remember decompression offset
        cld

        movzx   esi, si                 ; widen 16-bit args into ESI/EDI
        movzx   edi, di                 ; (high halves zero — small-model
                                        ; invariant; the depacker treats
                                        ; ESI/EDI as 32-bit pointers but
                                        ; never crosses the 64 KiB segment
                                        ; boundary in our use)

        xor     ecx, ecx
        xor     ebx, ebx                ; ebx = 0100h
        inc     bh
        xor     ebp, ebp

l2_decode_token:
        mul     ecx                     ; clear eax (ecx is 0 from above or after rep movsb)
        a32 lodsb                       ; token byte XYZ|LL|MMMM
        mov     dl, al                  ; keep token in dl

        and     al, 018h                ; isolate literals length (LL)
        shr     al, 3                   ; shift into place

        cmp     al, 03h                 ; LITERALS_RUN_LEN_V2?
        jne     l2_got_literals

        call    l2_get_nibble           ; extra literals length nibble
        add     al, cl
        cmp     al, 012h                ; LITERALS_RUN_LEN_V2 + 15?
        jne     l2_got_literals

        a32 lodsb                       ; extra length byte
        add     al, 012h                ; overflow?
        jnc     l2_got_literals

        a32 lodsw                       ; 16-bit extra length

l2_got_literals:
        xchg    ecx, eax
        a32 rep movsb                   ; copy literals ds:esi -> es:edi

        test    dl, 0C0h                ; check match-offset mode (X bit)
        js      l2_rep_match_or_large_offset

        xchg    ecx, eax                ; clear eax (ecx is 0 from rep movsb)
        jne     l2_offset_9_bit

                                        ; 5-bit offset
        cmp     dl, 020h                ; test bit 5
        call    l2_get_nibble_x
        jmp     l2_dec_offset_top

l2_offset_9_bit:                        ; 9-bit offset
        a32 lodsb                       ; 8-bit offset into AL
        dec     ah                      ; set offset bits 15-8 to 1
        test    dl, 020h                ; test bit Z (offset bit 8)
        je      l2_get_match_length
l2_dec_offset_top:
        dec     ah                      ; clear bit 8 if Z clear, else set offset bits 15-8
        jmp     l2_get_match_length

l2_rep_match_or_large_offset:
        jpe     l2_rep_match_or_16_bit

                                        ; 13-bit offset
        cmp     dl, 0A0h                ; test bit 5 (knowing bit 7 is set)
        xchg    ah, al
        call    l2_get_nibble_x
        sub     al, 2                   ; subtract 512
        jmp     l2_get_match_length_1

l2_rep_match_or_16_bit:
        test    dl, 020h                ; test bit Z (offset bit 8)
        jne     l2_repeat_match         ; rep-match

                                        ; 16-bit offset
        a32 lodsb                       ; 2-byte match offset

l2_get_match_length_1:
        xchg    ah, al
        a32 lodsb                       ; match offset bits 0-7

l2_get_match_length:
        xchg    ebp, eax                ; ebp = offset
l2_repeat_match:
        xchg    eax, edx                ; ax = original token
        and     al, 07h                 ; isolate match length (MMM)
        add     al, 2                   ; + MIN_MATCH_SIZE_V2

        cmp     al, 09h                 ; MIN_MATCH_SIZE_V2 + MATCH_RUN_LEN_V2?
        jne     l2_got_matchlen

        call    l2_get_nibble           ; extra match length nibble
        add     al, cl
        cmp     al, 018h                ; MIN_MATCH_SIZE_V2 + MATCH_RUN_LEN_V2 + 15?
        jne     l2_got_matchlen

        a32 lodsb                       ; extra length byte
        add     al, 018h                ; overflow?
        jnc     l2_got_matchlen
        je      l2_done                 ; EOD marker

        a32 lodsw                       ; 16-bit extra length

l2_got_matchlen:
        xchg    ecx, eax                ; ecx = match length
        xchg    esi, eax
        movsx   ebp, bp                 ; sign-extend bp -> ebp for negative offsets
        lea     esi, [ebp + edi]        ; esi = back-reference in output data
        a32 rep movsb                   ; copy match (ds:esi -> es:edi; es == ds)
        xchg    esi, eax                ; restore esi
        jmp     l2_decode_token

l2_done:
        pop     ax                      ; original di (low 16)
        xchg    di, ax                  ; di = original, ax = current
        sub     ax, di                  ; decompressed byte count -> AX

        pop     bp                      ; restore caller's bp
        pop     es                      ; restore caller's es
        ret

l2_get_nibble_x:
        cmc                             ; carry set if bit 4 was set
        rcr     al, 1
        call    l2_get_nibble           ; nibble for offset bits 0-3
        or      al, cl                  ; merge nibble
        rol     al, 1
        xor     al, 0E1h                ; set offset bits 7-5 to 1
        ret

l2_get_nibble:
        neg     bh                      ; nibble ready?
        jns     l2_has_nibble

        xchg    ebx, eax
        a32 lodsb                       ; load two nibbles
        xchg    ebx, eax

l2_has_nibble:
        mov     cl, 4                   ; swap high and low nibbles
        ror     bl, cl
        mov     cl, 0Fh
        and     cl, bl
        ret
