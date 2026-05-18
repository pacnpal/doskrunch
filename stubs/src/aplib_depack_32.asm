;  aplib_depack_32.asm — 32-bit-register aPLib depacker for the doskrunch
;  386 stub (bits 16, cpu 386). Linked into stubs/blobs/aplib_386.bin and
;  invoked from stubs/src/stub.c via the same `aplib_depack` symbol the
;  8086 port (stubs/src/aplib_depack_16.asm) exports — the Makefile picks
;  the .obj per tier.
;
;  Source lineage:
;    Ported from apultra's `asm/x86/aplib_x86_small.asm`, the 185-byte
;    size-optimized 32-bit decoder by Emmanuel Marty. Upstream copy lives
;    in this tree at
;      vendor/apultra/asm/x86/aplib_x86_small.asm
;    and round-trips the apultra-compressed stream emitted by the host
;    (`host/src/compress/aplib.rs`).
;
;  License: Zlib (see vendor/apultra/LICENSE.zlib.md). Compatible with
;  this project's MIT license.
;
;  Adaptations for doskrunch (16-bit real mode + 32-bit registers):
;    * `bits 16 / cpu 386`. Target is 16-bit real-mode DOS on a 386+ CPU.
;      NASM auto-emits 0x66 (operand-size) and 0x67 (address-size)
;      prefixes wherever 32-bit register names appear under bits 16, so
;      the assembled bytes are real-mode-safe even though the body is
;      written in 32-bit form. Disassembly check (use a flat binary —
;      `ndisasm` treats its input as raw bytes and would mis-decode OMF
;      object headers / fixup records as instructions):
;        nasm -f bin stubs/src/aplib_depack_32.asm -o /tmp/depack32.bin
;        ndisasm -b 16 /tmp/depack32.bin
;    * `_TEXT` segment `public class=CODE use16` so Open Watcom wlink
;      links this OMF object alongside Watcom-compiled C in a small-model
;      executable.
;    * Public symbol `aplib_depack` (no underscore — Watcom register-based
;      calling, declared on the C side via `#pragma aux ... "*"`). The
;      8086 port uses the same name; both depackers are ABI-equivalent.
;    * 16-bit Watcom small-model ABI: first arg in SI, second in DI,
;      return in AX. We `movzx esi, si` and `movzx edi, di` at entry to
;      widen the caller-passed near pointers into 32-bit registers — the
;      pointers themselves are still DS-relative 16-bit offsets (small
;      model), the movzx just ensures ESI/EDI high halves are zero so
;      [esi]/[edi] resolve to the same DS-relative bytes [si]/[di] would.
;      Don't expose 32-bit registers in the C-side `parm` clause — that'd
;      shift Watcom's ABI off small model.
;    * Save/restore ES at entry/exit. Watcom small model pins CS=DS=SS
;      but not ES; the depacker writes through ES:[EDI] so the caller
;      would otherwise need to set ES=DS. We absorb that here for a
;      simpler C-side declaration.
;    * Save/restore BP at entry/exit. Watcom small-model `-os` keeps a
;      frame pointer in BP and reads locals as `[bp-N]` after we return
;      — without this push/pop the surrounding function's frame dies
;      (same bug fix as PR #3's commit 9da3438; the 8086 port has the
;      identical save/restore).
;    * The upstream's size-optimized `call .init_get_bit / pop ebp`
;      trick doesn't translate to 16-bit real mode: a NEAR call pushes
;      2 bytes for IP, but `pop ebp` under bits 16 emits a 0x66 prefix
;      and pops 4 bytes — unbalancing the stack. Replaced with the
;      explicit `mov bp, apl_get_bit / call bp` pattern from the 8086
;      port. BP holds a 16-bit offset within CS, which is all `call bp`
;      needs.
;    * `push 3 / pop ebx` likewise doesn't translate (16-bit push, 32-bit
;      pop). Replaced with `mov ebx, 3`.
;    * `pushad`/`popad` at entry/exit replaced with the Watcom
;      `modify exact [ax bx cx dx si di]` declaration on the C side.
;      The pragma covers the 16-bit register halves; high halves of
;      EAX/EBX/ECX/EDX are treated as caller-clobbered because Watcom's
;      16-bit ABI doesn't preserve them across function calls.
;    * Internal labels prefixed with `apl_` (no NASM `.local` scoping
;      under `-f obj`).
;    * Return value: AX = decompressed byte count (16 bits — the host
;      bounds per-chunk uncompressed size to ≤16 KiB via
;      archive.rs::APLIB_CHUNK_INPUT so a chunk never produces > 64 KiB
;      and the result always fits in AX).
;
;  TRUST BOUNDARY: see the comment in stubs/src/stub.c above the
;  `aplib_depack` extern declaration — same caveats apply to this port.
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

        cpu     386
        bits    16

        segment _TEXT public class=CODE use16
        global  aplib_depack

aplib_depack:
        push    es                      ; preserve caller's es
        push    bp                      ; preserve Watcom's frame pointer
        push    ds
        pop     es                      ; es = ds (small-model data seg)

        push    di                      ; remember decompression offset
        cld

        movzx   esi, si                 ; widen 16-bit args into ESI/EDI
        movzx   edi, di                 ; (high halves zero — small-model
                                        ; invariant; the depacker treats
                                        ; ESI/EDI as 32-bit pointers but
                                        ; never crosses the 64 KiB segment
                                        ; boundary in our use)

        mov     al, 080h                ; bit queue primer
        xor     edx, edx                ; invalidate rep offset
        mov     bp, apl_get_bit         ; `call bp` dispatch addr

apl_literal:
        a32 movsb                       ; ds:[esi] -> es:[edi], advance
apl_next_command_after_literal:
        mov     ebx, 3                  ; follows_literal = 3

apl_next_command:
        call    bp                      ; 'literal or match' bit
        jnc     apl_literal

        call    bp                      ; '8+n bits or other' bit
        jc      apl_other

        call    apl_get_gamma2          ; ecx = gamma2 high offset bits
        sub     ecx, ebx                ; rep-match test
        jae     apl_not_repmatch

        call    apl_get_gamma2          ; rep-match length
        jmp     short apl_got_len

apl_not_repmatch:
        mov     edx, ecx
        shl     edx, 8
        mov     dl, [esi]               ; low offset byte (DS:[ESI])
        inc     esi

        call    apl_get_gamma2          ; match length
        cmp     edx, 07D00h
        jae     apl_increase_len_by2
        cmp     edx, 0500h
        jae     apl_increase_len_by1
        cmp     edx, 0080h
        jae     apl_got_len
apl_increase_len_by2:
        inc     ecx
apl_increase_len_by1:
        inc     ecx

apl_got_len:
        push    esi                     ; save current input pointer
        mov     esi, edi
        sub     esi, edx                ; match source = current dst - offset
        a32 rep movsb                   ; copy ecx bytes ds:[esi] -> es:[edi]
        pop     esi

        mov     bl, 02h                 ; follows_literal = 2 (only the low
                                        ; byte changes; ebx upper halves
                                        ; are irrelevant for the next
                                        ; gamma2 sub)
        jmp     short apl_next_command

apl_get_gamma2:
        xor     ecx, ecx
        inc     ecx

apl_gamma2_loop:
        call    apl_get_dibits
        jc      apl_gamma2_loop
        ret

apl_other:
        xor     ecx, ecx
        call    bp                      ; '7+1 match or short literal' bit
        jc      apl_short_literal

        movzx   edx, byte [esi]         ; 7-bit offset + 1-bit length
        inc     esi
        inc     ecx
        shr     dl, 1                   ; len bit -> carry, offset shifts down
        je      apl_done                ; zero offset = EOD
        adc     ecx, ecx                ; length = 2 or 3
        jmp     apl_got_len

apl_short_literal:
        call    apl_get_dibits          ; 4-bit short offset (2 x dibit)
        adc     ecx, ecx
        call    apl_get_dibits
        adc     ecx, ecx
        xchg    eax, ecx                ; ecx = saved bit queue, eax = offset
        jz      apl_write_zero          ; zero offset -> write a zero byte

        mov     ebx, edi                ; ebx = current dst - short offset
        sub     ebx, eax
        mov     al, [ebx]               ; source byte at short offset
apl_write_zero:
        a32 stosb                       ; write AL at es:[edi++]
        xchg    eax, ecx                ; restore bit queue in AL
        jmp     apl_next_command_after_literal

apl_done:
        pop     ax                      ; original di (low 16)
        xchg    di, ax                  ; di = original, ax = current
        sub     ax, di                  ; decompressed byte count -> AX
        pop     bp
        pop     es
        ret

apl_get_dibits:
        call    bp                      ; first bit -> CF
        adc     ecx, ecx                ; shift into ecx
        ; fall through: read continuation bit, leave CF for caller

apl_get_bit:
        add     al, al
        jnz     apl_got_bit
        a32 lodsb                       ; refill bit queue from DS:[ESI]
        adc     al, al
apl_got_bit:
        ret
