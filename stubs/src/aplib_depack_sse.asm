;  aplib_depack_sse.asm — SSE-accelerated 32-bit-register aPLib depacker
;  for the doskrunch p3 stub (bits 16, cpu katmai). Exports the same
;  `aplib_depack` symbol as the 8086/386/p5/mmx ports.
;
;  ===== NOT WIRED IN AS OF PHASE 5 =====
;  stubs/Makefile builds aplib_p3.bin against aplib_depack_mmx.obj, not
;  this file. Under DOSBox-X 2026.05.02 cputype=pentium_iii the
;  MOVUPS-based block-copy loop hangs on multi-chunk payloads bigger
;  than the small-fixture DOSBox-X gate. NASM disassembly of the loop
;  encoding looks correct (67 0F 10 06 / 67 0F 11 07 for MOVUPS xmm0,
;  [esi] / [edi], xmm0), so the symptom is most likely a DOSBox-X SSE
;  emulation gap rather than a depacker bug. A real Pentium III box or
;  a different emulator would prove or disprove that. Until then the
;  asm is kept on disk for follow-up; nothing links it. The p3 blob
;  still benefits from wcc -6 codegen for the surrounding C
;  housekeeping and the MMX path for length-8-or-longer matches.
;
;  Source lineage:
;    Forked from stubs/src/aplib_depack_mmx.asm. Identical bitstream
;    decoder; the only difference is the match-copy path uses MOVUPS
;    (16 bytes per iter) when offset >= 16 instead of MOVQ (8 bytes
;    per iter). The MMX fallback isn't carried in — the gate-down
;    cost (cmp + jb) twice would dwarf the MMX win on the small
;    window between length 8..15.
;
;  License: Zlib.
;
;  What SSE buys us:
;    Same caveat as the MMX port — aPLib's stream has no "literal run
;    of length N" opcode, so SSE only accelerates the match-copy hot
;    path:
;
;      if length >= 16 && offset >= 16: copy 16 bytes per MOVUPS, tail with movsb
;      else:                             scalar rep movsb (same as p5 port)
;
;    MOVUPS (vs MOVAPS) doesn't require 16-byte aligned dest, which
;    matters in our small-model stub where g_buf isn't aligned beyond
;    word. Aligning g_buf would need a NASM `alignb=16` directive plus
;    Watcom linker cooperation, and the MOVUPS micro-op latency on a
;    P3 is only ~1 cycle worse than MOVAPS on aligned data; not worth
;    the link-side surgery.
;
;    EMMS is NOT needed here — SSE registers (XMM0..XMM7) don't share
;    state with x87. We still skip MMX entirely on this port so EMMS
;    never enters the picture.
;
;  Adaptations for doskrunch: identical to aplib_depack_p5.asm + the
;  MMX/SSE notes above. `cpu katmai` (NASM's name for the Pentium III
;  microarchitecture) is the lowest CPU directive that accepts SSE
;  instructions like MOVUPS — `cpu pentium` rejects them outright
;  ("no instruction for this cpu level"). Katmai matches our runtime
;  target.
;
;  TRUST BOUNDARY: see stubs/src/stub.c above the `aplib_depack` extern
;  declaration — same caveats apply.

        cpu     katmai
        bits    16

        segment _TEXT public class=CODE use16
        global  aplib_depack

%macro apl_get_bit_inline 0
        ; Bit-queue invariant matches aplib_depack_p5.asm — see that
        ; file's macro comment for the full explanation.
        add     al, al
        jnz     %%gotbit
        a32 lodsb
        adc     al, al
%%gotbit:
%endmacro

aplib_depack:
        push    es
        push    bp
        push    ds
        pop     es

        push    di
        cld

        movzx   esi, si
        movzx   edi, di

        mov     al, 080h
        xor     edx, edx

apl_literal:
        a32 movsb
apl_next_command_after_literal:
        mov     ebx, 3

apl_next_command:
        apl_get_bit_inline
        jnc     apl_literal

        apl_get_bit_inline
        jc      apl_other

        call    apl_get_gamma2
        sub     ecx, ebx
        jae     apl_not_repmatch

        call    apl_get_gamma2
        jmp     short apl_got_len

apl_not_repmatch:
        mov     edx, ecx
        shl     edx, 8
        mov     dl, [esi]
        inc     esi

        call    apl_get_gamma2
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
        sub     esi, edx                ; match source = dst - offset
        ; SSE gate: length >= 16 AND offset >= 16 means dst and src
        ; windows of `length` bytes don't overlap, so MOVUPS is
        ; correctness-safe. Either side smaller falls through to
        ; scalar rep movsb (no MMX intermediate — see header).
        cmp     ecx, 16
        jb      apl_match_scalar
        cmp     edx, 16
        jb      apl_match_scalar

        ; SSE block-copy. Iterate while ECX >= 16 copying 16 bytes per
        ; MOVUPS, then tail with `rep movsb` for the remainder.
apl_match_sse_loop:
        a32 movups      xmm0, [esi]
        a32 movups      [edi], xmm0
        a32 add         esi, 16
        a32 add         edi, 16
        sub     ecx, 16
        cmp     ecx, 16
        jae     apl_match_sse_loop

apl_match_scalar:
        a32 rep movsb
        pop     esi

        mov     bl, 02h
        ; Near jump: the SSE block above pushed the back-edge past the
        ; 127-byte short-jump range. Costs one byte over `jmp short`.
        jmp     apl_next_command

apl_get_gamma2:
        xor     ecx, ecx
        inc     ecx

apl_gamma2_loop:
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        jc      apl_gamma2_loop
        ret

apl_other:
        xor     ecx, ecx
        apl_get_bit_inline
        jc      apl_short_literal

        movzx   edx, byte [esi]
        inc     esi
        inc     ecx
        shr     dl, 1
        je      apl_done
        adc     ecx, ecx
        jmp     apl_got_len

apl_short_literal:
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        apl_get_bit_inline
        adc     ecx, ecx
        xchg    eax, ecx
        jz      apl_write_zero

        mov     ebx, edi
        sub     ebx, eax
        mov     al, [ebx]
apl_write_zero:
        a32 stosb
        xchg    eax, ecx
        jmp     apl_next_command_after_literal

apl_done:
        ; No EMMS — this port uses SSE (XMM regs), not MMX.
        pop     ax
        xchg    di, ax
        sub     ax, di
        pop     bp
        pop     es
        ret
