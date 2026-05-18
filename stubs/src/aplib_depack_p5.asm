;  aplib_depack_p5.asm — speed-optimized 32-bit-register aPLib depacker
;  for the doskrunch pentium stub (bits 16, cpu pentium). Linked into
;  stubs/blobs/aplib_pentium.bin. Exports the same `aplib_depack` symbol
;  as the 8086 and 386 ports; the Makefile picks the .obj per tier.
;
;  Source lineage:
;    Ported from apultra's `asm/x86/aplib_x86_fast.asm`, the 188-byte
;    speed-optimized 32-bit decoder by Emmanuel Marty. Upstream copy
;    lives at vendor/apultra/asm/x86/aplib_x86_fast.asm. The two
;    upstream variants differ as follows:
;      * Inlines `apl_get_bit` at every call site via a macro (no
;        call/ret pair on the hot bitstream-read path).
;      * Replaces `push 3 / pop ebx` (size-opt) with `mov ebx, 3` (speed
;        — avoids the push/pop dependency chain).
;      * Reads the 4-bit short literal offset as 4 single-bit reads
;        instead of 2 dibit subroutine calls.
;
;  License: Zlib.
;
;  Adaptations for doskrunch: identical to aplib_depack_32.asm — see
;  that file's header block for the full list (16-bit real mode +
;  32-bit-register port, ES/BP save/restore, movzx widening, a32 on
;  string ops, `mov bp,…/call bp` replacing the upstream's `call/pop`
;  size trick, etc.). Additionally:
;    * `cpu pentium` (vs `cpu 386` in aplib_depack_32.asm). No
;      P5-specific instructions are emitted; the directive is
;      informational and matches the Watcom `-5` flag used to compile
;      stub.c into the pentium stub.
;    * `apl_get_bit_inline` macro replaces the `call bp` indirect
;      dispatch from the size-opt port. Each macro expansion is 8 bytes
;      under bits 16 + cpu pentium: `add al,al` (00 C0), `jnz %%gotbit`
;      (75 XX), `a32 lodsb` (67 AC), `adc al,al` (10 C0) — 4 × 2 = 8.
;      The macro costs code size at every bit read but removes a
;      call/ret pair and a register-indirect branch from the hot path.
;    * Manual U/V pipe scheduling is *not* applied in this revision.
;      PLAN.md §10 Phase 3 anticipates a hand-scheduled depacker (5–10×
;      speedup over 8086). The unscheduled fast variant is benchmarked
;      first; if numbers miss the gate, a follow-up adds U/V annotations
;      and reorders. Karpathy: measure before optimizing.
;
;  TRUST BOUNDARY: see stubs/src/stub.c above the `aplib_depack` extern
;  declaration — same caveats apply.
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

        cpu     pentium
        bits    16

        segment _TEXT public class=CODE use16
        global  aplib_depack

%macro apl_get_bit_inline 0
        ; Bit-queue invariant (matches the 8086 port and apultra's x86
        ; size-opt variant): AL holds 7 unread data bits in the high
        ; bits and a 1-sentinel somewhere below. Each `add al, al` shifts
        ; the next data bit into CF and the sentinel toward MSB.
        ;
        ; Falling through to the refill path means `add al, al` produced
        ; AL == 0 (so jnz didn't fire) — which happens exactly when the
        ; sentinel was the only 1 still in AL and just shifted out. That
        ; sentinel sits in CF now (CF=1). LODSB doesn't touch flags, so
        ; CF survives the load; the trailing `adc al, al` then computes
        ;   AL = (AL << 1) + CF  (= (next_byte << 1) | 1)
        ; which delivers the new bit in CF (from `next_byte`'s MSB) and
        ; restores the 1-sentinel into AL bit 0.
        add     al, al                  ; data bit -> CF; AL shifts left
        jnz     %%gotbit                ; AL nonzero -> queue still has bits
        a32 lodsb                       ; AL == 0: refill from DS:[ESI++].
                                        ; Flags preserved -> CF=1 (sentinel)
        adc     al, al                  ; CF (data bit) -> CF, old CF=1
                                        ; becomes the new sentinel in bit 0
%%gotbit:
%endmacro

aplib_depack:
        push    es                      ; preserve caller's es
        push    bp                      ; preserve Watcom's frame pointer
        push    ds
        pop     es                      ; es = ds (small-model data seg)

        push    di                      ; remember decompression offset
        cld

        movzx   esi, si                 ; widen 16-bit args into ESI/EDI
        movzx   edi, di

        mov     al, 080h                ; bit queue primer
        xor     edx, edx                ; invalidate rep offset

apl_literal:
        a32 movsb                       ; ds:[esi] -> es:[edi]
apl_next_command_after_literal:
        mov     ebx, 3                  ; follows_literal = 3

apl_next_command:
        apl_get_bit_inline              ; 'literal or match' bit
        jnc     apl_literal

        apl_get_bit_inline              ; '8+n bits or other' bit
        jc      apl_other

        call    apl_get_gamma2          ; ecx = gamma2 high offset bits
        sub     ecx, ebx                ; rep-match test
        jae     apl_not_repmatch

        call    apl_get_gamma2          ; rep-match length
        jmp     short apl_got_len

apl_not_repmatch:
        mov     edx, ecx
        shl     edx, 8
        mov     dl, [esi]
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
        push    esi
        mov     esi, edi
        sub     esi, edx
        a32 rep movsb
        pop     esi

        mov     bl, 02h                 ; follows_literal = 2
        jmp     short apl_next_command

apl_get_gamma2:
        xor     ecx, ecx
        inc     ecx

apl_gamma2_loop:
        apl_get_bit_inline              ; data bit
        adc     ecx, ecx
        apl_get_bit_inline              ; continuation bit
        jc      apl_gamma2_loop
        ret

apl_other:
        xor     ecx, ecx
        apl_get_bit_inline              ; '7+1 match or short literal' bit
        jc      apl_short_literal

        movzx   edx, byte [esi]
        inc     esi
        inc     ecx
        shr     dl, 1                   ; len bit -> carry, offset shifts down
        je      apl_done                ; zero offset = EOD
        adc     ecx, ecx                ; length = 2 or 3
        jmp     apl_got_len

apl_short_literal:
        apl_get_bit_inline              ; 4-bit short offset (4 x bit)
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        xchg    eax, ecx                ; ecx = saved bit queue, eax = offset
        jz      apl_write_zero          ; zero offset -> write a zero byte

        mov     ebx, edi
        sub     ebx, eax
        mov     al, [ebx]
apl_write_zero:
        a32 stosb
        xchg    eax, ecx                ; restore bit queue in AL
        jmp     apl_next_command_after_literal

apl_done:
        pop     ax                      ; original di (low 16)
        xchg    di, ax
        sub     ax, di                  ; decompressed byte count -> AX
        pop     bp
        pop     es
        ret
