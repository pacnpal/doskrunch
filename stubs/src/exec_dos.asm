;  exec_dos.asm — DOS INT 21h/4Bh EXEC wrapper (v1.1 run-after-extract).
;
;  Loads and executes a named DOS program. Control returns only when the
;  child process exits (or immediately on EXEC failure). After return the
;  caller should exit; this wrapper does not propagate the child's
;  errorlevel (out-of-scope per the v1.1 design note in tasks/todo.md).
;
;  SS:SP save/restore:
;    DOS EXEC (INT 21h/4Bh AL=0) destroys the parent program's SS:SP
;    registers (and potentially all GP registers) across the call. The
;    standard fix: save SS, SP, and DS into words in the CODE segment
;    (accessible via CS: override at any time, including when SS is
;    wrong) before the call, and restore them afterward.
;
;  Calling convention — Watcom small-model register-based (same as the
;  aplib/lzsa2 depackers):
;    parm [si]   near offset in DS of the NUL-terminated program path
;    parm [di]   near offset in DS of the 14-byte EXEC parameter block
;    value [ax]  0 on success, DOS error code (nonzero) on failure
;    modify exact [ax bx cx dx si di]
;    (bp, ds, es, ss, sp are all saved/restored by this wrapper; the
;     caller's pragma should NOT list them in modify.)
;
;  This file is compiled to exec_dos.obj by NASM -f obj and linked into
;  every tier blob (aplib_*.bin and lzma_*.bin) via the Makefile rules.
;  cpu 8086 is intentional: exec_dos must run on every supported tier.

        cpu     8086
        bits    16

        segment _TEXT public class=CODE use16
        global  exec_dos

;  ---------------------------------------------------------------------------
;  void exec_dos(u16 prog_si, u16 pb_di);
;
;  On entry:
;    SI = near DS offset of NUL-terminated program path (DS:SI → path)
;    DI = near DS offset of 14-byte EXEC parameter block (DS:DI → pb)
;
;  On exit:
;    AX = 0  if the program was loaded, ran, and exited (carry clear on
;            return from INT 21h/4Bh)
;    AX ≠ 0  if DOS returned an error loading the program (carry set;
;            AX = DOS error code)
;
;  Caller-saved (by this wrapper): BP, DS, ES
;  Truly clobbered: AX, BX, CX, DX, SI, DI
;  ---------------------------------------------------------------------------

exec_dos:
        push    es                      ; preserve caller's ES
        push    bp                      ; preserve caller's BP (Watcom frame)

        ; Save SS, SP, DS before issuing INT 21h/4Bh.
        ; CS: override lets us reach these words even when SS is wrong.
        cli
        mov     word [cs:saved_ss], ss
        mov     word [cs:saved_sp], sp  ; sp here includes the pushed ES+BP above
        mov     word [cs:saved_ds], ds
        sti

        ; ES:BX → EXEC parameter block (DS:DI — same DS, so ES = DS).
        push    ds
        pop     es
        mov     bx, di

        ; DS:DX → NUL-terminated program path (DS:SI — same DS).
        mov     dx, si

        ; INT 21h / AH=4Bh, AL=0 — Load and Execute.
        mov     ax, 0x4B00
        int     0x21

        ; On return from INT 21h/4Bh:
        ;   CS:IP  — correct (DOS restores the instruction pointer)
        ;   SS:SP  — may be destroyed; restore from our CS-relative save
        ;   DS, ES, AX, BX, CX, DX, SI, DI, BP — may all be trashed

        ; Restore SS and SP (interrupts off while they are inconsistent).
        cli
        mov     ss, [cs:saved_ss]
        mov     sp, [cs:saved_sp]
        sti

        ; Stack is now valid; restore DS.
        mov     ds, [cs:saved_ds]

        ; Restore BP and ES from the now-correct stack.
        pop     bp
        pop     es

        ; CF clear → program ran to completion; return 0 in AX.
        ; CF set  → EXEC failed; AX already holds the DOS error code.
        jnc     .ok
        ret                             ; return AX (nonzero = error code)
.ok:
        xor     ax, ax                  ; return 0 on success
        ret

; Words in the CODE segment used for SS/SP/DS save/restore.
; Placed after all ret paths — execution never reaches here.
saved_ss:   dw  0
saved_sp:   dw  0
saved_ds:   dw  0
