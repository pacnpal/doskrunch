;  rdtsc_helper.asm — Reusable RDTSC cycle-counter primitive for pentium+
;  tier stubs (bits 16, cpu pentium). Callable from Watcom 16-bit small-
;  model C via the `#pragma aux rdtsc_lo` declaration in stub.c.
;
;  Exported symbol:
;    rdtsc_lo  — reads the IA32 Time Stamp Counter (RDTSC) and returns
;                the low 32 bits as a Watcom 16-bit unsigned long (DX:AX).
;
;  Usage in C (small model, 16-bit):
;    extern unsigned long rdtsc_lo(void);
;    #pragma aux rdtsc_lo "*" value [dx ax] modify exact [ax dx];
;
;    unsigned long t0 = rdtsc_lo();
;    /* ... work to time ... */
;    unsigned long elapsed = rdtsc_lo() - t0;  /* unsigned wrap-around correct */
;
;  Notes:
;    * RDTSC is a Pentium (P5) class instruction. Do NOT link rdtsc_helper.obj
;      into 8086 / 286 / 386 / 486 tier stubs — those CPUs don't have RDTSC
;      and will fault with an invalid-opcode exception at runtime.
;    * Only the low 32 bits of the 64-bit TSC are returned. At 100 MHz
;      (vintage pentium-mmx), 2^32 cycles wrap in ~42 seconds. A 500 KiB
;      aPLib decode takes well under 1 second even on the slowest emulated
;      pentium, so a 32-bit accumulator is sufficient.
;    * RDTSC is not serializing on P5 / P6. Surrounding instructions may
;      reorder past it. For decode-loop measurements spanning millions of
;      cycles this is negligible. If sub-microsecond accuracy is needed,
;      use CPUID as a serializing fence before each RDTSC.
;    * Reused across issues #13 (LZMA-vs-aPLib timing gate) and #14
;      (Phase 3 386/pentium speedup gate). All three issues instrument the
;      same `aplib_depack` call site in stub.c; the shared helper keeps
;      stub size impact to a minimum (~20 bytes of code).
;
;  Watcom 16-bit calling convention for `unsigned long rdtsc_lo(void)`:
;    * No arguments (no registers passed in).
;    * Return: DX:AX where DX = high 16 bits, AX = low 16 bits.
;    * Clobbers: AX, DX (the return registers only).
;    * Preserves: BX, CX, SI, DI, BP, ES.

        cpu     pentium
        bits    16

        segment _TEXT public class=CODE use16
        global  rdtsc_lo

rdtsc_lo:
        ; RDTSC: EAX = TSC[31:0], EDX = TSC[63:32].
        ; We ignore EDX (high 64-bit half) — only the low 32 bits are returned.
        rdtsc

        ; Decompose EAX into Watcom's DX:AX return convention:
        ;   DX = EAX[31:16]  (high word)
        ;   AX = EAX[15:0]   (low word, already in AX after RDTSC)
        ; Strategy: copy EAX to EDX, shift right 16, so DX holds the high half.
        ; AX is already the low 16 bits of EAX (AX is the low word of EAX always).
        mov     edx, eax        ; EDX = EAX (copy full 32-bit TSC low)
        shr     edx, 16         ; DX  = EAX[31:16]

        ; AX = EAX[15:0] (unchanged — RDTSC wrote EAX, AX is its low 16 bits)
        ; DX = EAX[31:16]
        ; => DX:AX = EAX = TSC[31:0]  (Watcom unsigned long return convention)
        ret
