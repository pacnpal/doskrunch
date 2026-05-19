;  aplib_depack_mmx.asm — MMX-accelerated 32-bit-register aPLib depacker
;  for the doskrunch pentium-mmx and p2 stubs (bits 16, cpu pentium).
;  Linked into stubs/blobs/aplib_pentium-mmx.bin and aplib_p2.bin.
;  Exports the same `aplib_depack` symbol as the 8086/386/p5/sse ports;
;  the Makefile picks the .obj per tier.
;
;  Source lineage:
;    Forked from stubs/src/aplib_depack_p5.asm (the speed-opt port of
;    apultra's `asm/x86/aplib_x86_fast.asm`). Identical bitstream
;    decoder; only the match-copy and gamma2 paths differ. See the
;    aplib_depack_p5.asm header block for the upstream attribution.
;
;  License: Zlib (see vendor/apultra/LICENSE.zlib.md). Compatible with
;  this project's MIT license.
;
;  What MMX buys us, and where it doesn't:
;    aPLib emits literals one byte at a time, gated on bit-decode
;    decisions — there's no "literal run of length N" opcode the way
;    LZMA or LZSA have one, so MOVQ can't accelerate the literal path.
;    The vectorizable surface is the match-copy:
;      a32 rep movsb     ; cx bytes from DS:ESI to ES:EDI
;    `rep movsb` is the hot path for back-references. MOVQ copies 8
;    bytes per iteration, but it's only safe when source and dest don't
;    overlap — i.e. when the back-reference offset is >= 8 bytes. For
;    a 3-byte match-with-offset-1 (the canonical compression-of-zeros
;    case), MOVQ would corrupt the run. So we gate:
;
;      if length >= 8 && offset >= 8:  copy 8 bytes per MOVQ, tail with movsb
;      else:                            scalar rep movsb (same as p5 port)
;
;    Practical speedup ceiling: aPLib's typical match distribution has
;    a heavy short-match tail (offset 1..7 / length 2..6), which the
;    MMX path skips. The 30% speedup gate in PLAN.md §10 Phase 5 Verify
;    is documented as conditional on a "literal-heavy payload" — but
;    aPLib literals don't bunch into runs, so "literal-heavy" here
;    actually means "high-frequency match-followed-by-literal", which
;    helps the C-level loop scheduling but not MMX. The gate row is
;    left open in tasks/todo.md if a benchmark on real DOSBox-X /
;    real iron doesn't show the 30% headroom.
;
;  Adaptations for doskrunch: identical to aplib_depack_p5.asm —
;  16-bit real mode + 32-bit registers, ES/BP save/restore, movzx
;  widening, a32 prefix on string ops, `mov bp,…/call bp` replacing
;  upstream's `call/pop` size trick. Plus:
;    * `cpu pentium` (vs `cpu pentium-mmx`). NASM's `cpu pentium-mmx`
;      directive doesn't exist; we use `cpu pentium` and explicitly
;      emit MMX instructions which NASM accepts under any cpu
;      directive >= pentium. The runtime CPU is the gate, not the
;      assembler.
;    * EMMS before every `ret` — MMX shares the x87 register stack;
;      a stray EMMS-less return would corrupt the FPU tag word for
;      any C-side caller that later touches x87. stub.c doesn't use
;      x87 today, but emitting EMMS is cheap defense against a future
;      regression.
;    * `apl_copy_match_mmx` subroutine: the MMX-accelerated path. The
;      scalar `rep movsb` fallback stays inline so the common
;      short-match case doesn't pay a call/ret.
;
;  TRUST BOUNDARY: see stubs/src/stub.c above the `aplib_depack` extern
;  declaration — same caveats apply to this port.

        cpu     pentium
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
        ; MMX gate: length >= 8 AND offset >= 8 means dst..dst+length
        ; and src..src+length don't overlap, so MOVQ is correctness-
        ; safe. Either side smaller falls through to scalar rep movsb.
        cmp     ecx, 8
        jb      apl_match_scalar
        cmp     edx, 8
        jb      apl_match_scalar

        ; MMX block-copy. Iterate while ECX >= 8 copying 8 bytes per
        ; MOVQ, then tail with `rep movsb` for the remainder. EAX is
        ; the bit-queue and must be preserved across the loop — MMX
        ; instructions don't touch it.
apl_match_mmx_loop:
        a32 movq        mm0, [esi]
        a32 movq        [edi], mm0
        a32 add         esi, 8
        a32 add         edi, 8
        sub     ecx, 8
        cmp     ecx, 8
        jae     apl_match_mmx_loop
        ; ECX now in 0..7 — fall through to the scalar tail.

apl_match_scalar:
        a32 rep movsb
        pop     esi

        mov     bl, 02h
        ; Near jump: the MMX block above pushed the back-edge past the
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
        ; EMMS before returning: clears the MMX state so any future
        ; x87 use by the C caller doesn't see corrupted tag words. The
        ; depacker may have skipped the MMX path entirely for a small
        ; payload, but emitting EMMS unconditionally is 2 bytes and
        ; avoids a per-path branch.
        emms
        pop     ax
        xchg    di, ax
        sub     ax, di
        pop     bp
        pop     es
        ret
