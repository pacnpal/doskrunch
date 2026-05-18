;  aplib_depack_16.asm - 16-bit aPLib depacker for the doskrunch 8086 stub.
;
;  Source lineage:
;    Ported byte-for-byte from apultra's `asm/8088/aplib_8088_small.S`,
;    the 145-byte size-optimized 8088 decoder by Emmanuel Marty.
;    Upstream copy lives in this tree at
;      vendor/apultra/asm/8088/aplib_8088_small.S
;    and is the reference implementation that round-trips apultra's
;    compressor output (the stream emitted by the host's
;    `host/src/compress/aplib.rs`).
;
;  License: Zlib license (see vendor/apultra/LICENSE.zlib.md and the
;  attribution block below). Compatible with this project's MIT license.
;
;  Adaptations for doskrunch:
;    * `_TEXT` segment with explicit `class=CODE use16` so Open Watcom
;      wlink can link this OMF object alongside Watcom-compiled C in the
;      same small-model executable.
;    * Public symbol `aplib_depack` (no underscore — Watcom register-
;      based calling, declared on the C side via `#pragma aux`).
;    * Save/restore ES at entry/exit. Watcom small model pins CS=DS=SS
;      but does *not* pin ES; the depacker writes through ES:DI so the
;      caller would otherwise need to know to set ES=DS. We absorb that
;      here for a simpler C-side declaration.
;    * Internal labels prefixed with `apl_` so future link-time
;      collisions stay obvious (NASM scoping under `apl_decompress.foo`
;      is replaced by explicit names that survive `nasm -f obj`).
;
;  -- begin upstream attribution --
;  Copyright (C) 2019 Emmanuel Marty
;
;  This software is provided 'as-is', without any express or implied
;  warranty.  In no event will the authors be held liable for any damages
;  arising from the use of this software.
;
;  Permission is granted to anyone to use this software for any purpose,
;  including commercial applications, and to alter it and redistribute it
;  freely, subject to the following restrictions:
;
;  1. The origin of this software must not be misrepresented; you must not
;     claim that you wrote the original software. If you use this software
;     in a product, an acknowledgment in the product documentation would be
;     appreciated but is not required.
;  2. Altered source versions must be plainly marked as such, and must not be
;     misrepresented as being the original software.
;  3. This notice may not be removed or altered from any source distribution.
;  -- end upstream attribution --

        cpu     8086
        bits    16

        segment _TEXT public class=CODE use16
        global  aplib_depack

;  ---------------------------------------------------------------------------
;  Decompress aPLib data.
;
;  C-side declaration (small model, register-based calling):
;    unsigned int aplib_depack(const char *src, char *dst);
;    #pragma aux aplib_depack parm [si] [di] value [ax] modify [bx cx dx bp];
;
;  inputs:
;    ds:si  compressed aPLib stream
;    es:di  destination buffer (this wrapper sets es=ds on entry; caller
;           only needs to put dst in di)
;
;  output:
;    ax     decompressed size in bytes
;
;  Trashes: ax, bx, cx, dx, si, di, bp, flags. es is preserved.
;  ---------------------------------------------------------------------------

aplib_depack:
        push    es                      ; preserve caller's es
        push    ds
        pop     es                      ; es = ds (small-model data segment)

        push    di                      ; remember decompression offset
        cld

        mov     al,080h
        xor     dx,dx
        mov     bp,apl_get_bit

apl_literal:
        movsb
apl_next_command_after_literal:
        mov     bx,03h

apl_next_command:
        call    bp
        jnc     apl_literal

        call    bp
        jc      apl_other

        call    apl_get_gamma2
        sub     cx,bx
        jae     apl_not_repmatch

        call    apl_get_gamma2
        jmp     short apl_got_len

apl_not_repmatch:
        mov     dh,cl
        mov     dl,[si]
        inc     si

        call    apl_get_gamma2
        cmp     dh,07dh
        jae     apl_increase_len_by2
        cmp     dh,05h
        jae     apl_increase_len_by1
        cmp     dx,0080h
        jae     apl_got_len
apl_increase_len_by2:
        inc     cx
apl_increase_len_by1:
        inc     cx

apl_got_len:
        push    ds
        push    si

        push    es
        pop     ds
        mov     si,di
        sub     si,dx
        rep     movsb

        pop     si
        pop     ds

        mov     bl,02h
        jmp     short apl_next_command

apl_get_gamma2:
        xor     cx,cx
        inc     cx

apl_gamma2_loop:
        call    apl_get_dibits
        jc      apl_gamma2_loop
        ret

apl_other:
        xor     cx,cx
        call    bp
        jc      apl_short_literal

        mov     dl,[si]
        inc     si

        inc     cx
        shr     dl,1
        je      apl_done
        adc     cx,cx

        xor     dh,dh
        jmp     short apl_got_len

apl_short_literal:
        call    apl_get_dibits
        adc     cx,cx
        call    apl_get_dibits
        adc     cx,cx
        xchg    ax,cx
        jz      apl_write_zero

        mov     bx,di
        sub     bx,ax
        mov     al,[es:bx]
apl_write_zero:
        stosb
        xchg    ax,cx
        jmp     apl_next_command_after_literal

apl_done:
        pop     ax                      ; original di
        xchg    di,ax
        sub     ax,di                   ; decompressed size in ax
        pop     es                      ; restore caller's es
        ret

apl_get_dibits:
        call    bp
        adc     cx,cx

apl_get_bit:
        add     al,al
        jnz     apl_got_bit
        lodsb
        adc     al,al
apl_got_bit:
        ret
