| lfo.dks, the hooks of the UI task (both devices, the same code): the name of a shape on the MOD page, and the maximum of the WAVE parameter so that the encoder reaches the shapes.
| They are `.dks` hooks (`HOOKS` of the service, bound by `patchkit bind` to the firmware it runs on), not interrupt slots: the UI task runs them, one at a time. Each runs in place of the first
| 6 bytes of a stock function and leaves by `jmp` to `lfo_x_*`, the end of its site (`[object.externs]` of module.toml, the hooks of devices/fw/lfo.toml). With no shape loaded, the stock code
| runs unchanged. Register facts (docs/findings/143-lfo-waveforms.md, "The panel hooks", from `tools/coldfire/liveness.py` on the stock bytes): at the entry of each function the displaced
| instructions leave d0 (and for the range accessor d1) written before they are read, and the flags dead; every other register the routines use is saved.

    .text
    .balignw 4, 0x4e71

| The name formatters `fmt(int value, char *buf)`: sprintf(buf, "%s", name[value >> 8]) for the seven stock waves, sprintf(buf, "ERR") above them. Entry: 4(sp) = value, 8(sp) = buf.
| The displaced instructions are `link.w a6,#-0x1c; moveq #6,d0` (6 bytes). For a value of 0x700 to 0xeff (the wave numbers 7 to 14) with a loaded shape, `dk_lfo_name(value, buf, width)` writes
| the name (`width` characters at most) and the NUL, and the routine returns the number of characters in d0, as the stock sprintf does. Any other value, and a number with no shape, runs the stock code.
| d1, a0 and a1 (the callee's scratch registers) are saved around the call, so the routine leaves what the stock code leaves untouched until its own calls.
    .macro lfo_name_hook entry, width, exit
    .globl \entry
\entry:
    move.l  4(%sp),%d0
    cmpi.l  #0x700,%d0
    blt.b   9f
    cmpi.l  #0xeff,%d0
    bgt.b   9f
    lea     -12(%sp),%sp
    movem.l %d1/%a0/%a1,(%sp)           | value now at 16(sp), buf at 20(sp)
    move.l  #\width,-(%sp)              | argument 3
    move.l  24(%sp),-(%sp)              | argument 2: buf
    move.l  24(%sp),-(%sp)              | argument 1: value
    jsr     dk_lfo_name
    lea     12(%sp),%sp
    movem.l (%sp),%d1/%a0/%a1
    lea     12(%sp),%sp
    tst.l   %d0
    beq.b   9f
    rts
9:  link.w  %a6,#-0x1c                  | the displaced instructions
    moveq   #6,%d0
    jmp     \exit
    .endm

    lfo_name_hook lfo_ui_name4, 4, lfo_x_name4
    lfo_name_hook lfo_ui_name3, 3, lfo_x_name3

| The range accessor `void *range(int code)` with A0 = the buffer for 12 bytes min, max and default copied from the code's T1 row (offset 8): it returns the buffer in d0. Entry: 4(sp) = code.
| The displaced instructions are `move.l d2,-(sp); move.l 8(sp),d1` (6 bytes). The codes 79, 89 and 99 are the WAVE parameter of the three LFOs: the routine calls the stock accessor (`lfo_stock_range`: the
| displaced instructions and the stock code from behind them, with the code pushed again for the extra return address), and with a shape loaded replaces the maximum (offset 4 of the buffer) by
| `dk_lfo_range_max()`, the highest loaded wave number << 8. Any other code runs the stock code. The registers the stock accessor leaves (d1, a1) are saved around the Rust call.
| The hook never runs in interrupt context: the status register is read first, and a non-zero interrupt mask (bits 8 to 10; an interrupt handler runs at its own level, the tasks at level 0,
| and the runtime's critical sections at 7) sends the call to the stock code. d1 is dead here (the displaced `move.l 8(SP),D1` writes it) and the flags too.
    .globl lfo_ui_range
lfo_ui_range:
    move.w  %sr,%d1
    andi.l  #0x700,%d1
    bne.b   9f
    move.l  4(%sp),%d0
    cmpi.l  #79,%d0
    beq.b   1f
    cmpi.l  #89,%d0
    beq.b   1f
    cmpi.l  #99,%d0
    bne.b   9f
1:  move.l  %d0,-(%sp)                  | the code again: the stock path reads it at 8(sp) behind its return address
    jsr     lfo_stock_range
    addq.l  #4,%sp
    movea.l %d0,%a0                     | the buffer
    lea     -12(%sp),%sp
    movem.l %d1/%a0/%a1,(%sp)
    jsr     dk_lfo_range_max
    movem.l (%sp),%d1/%a0/%a1
    lea     12(%sp),%sp
    tst.l   %d0
    beq.b   2f                          | no shape loaded: the stock maximum
    move.l  %d0,4(%a0)
2:  move.l  %a0,%d0
    rts
9:  move.l  %d2,-(%sp)                  | the displaced instructions
    move.l  8(%sp),%d1
    jmp     lfo_x_range
