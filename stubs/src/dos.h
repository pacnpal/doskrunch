/* dos.h — minimal types and small helpers used by the stub.
 *
 * Open Watcom v2 ships its own <dos.h> with INT 21h wrappers (_dos_open,
 * _dos_creat, _dos_read, _dos_write, _dos_close, _dos_setftime). We use
 * those directly from stub.c — no point shadowing them.
 *
 * Small memory model: all pointers are near (offset only); the C runtime
 * sets DS=SS=our data segment at startup.
 */
#ifndef DOSKRUNCH_DOS_H
#define DOSKRUNCH_DOS_H

#include <dos.h>
#include <stddef.h>

typedef unsigned char  u8;
typedef unsigned short u16;
typedef unsigned long  u32;
typedef short          i16;
typedef long           i32;

#endif
