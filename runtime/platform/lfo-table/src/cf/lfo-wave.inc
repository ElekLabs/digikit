| core/lfo-table: the wave stubs, the cells they read, the interrupt mask and the info table. The first lines of the device file's numbers (LFO_WAVES) come from
| device-<dev>.inc (`patchkit gen`), included by lfo-wave-<dev>.s.
|
| The LFO tick and its second reader call `lfo_fn_table[wave]` with the phase (u32) at 4(SP) and take the value, signed Q31, from D0. A stub writes only D0, D1, A0 and the flags,
| makes no call, uses no stack and no MAC register. The value is the formula of services/lfo/src/shapes.rs `interp_cells`:
|     i = phase >> 27, f = (phase >> 12) & 0x7fff, value = cell[i].v + ((cell[i].d * f) << 1)   (wrapping 32-bit arithmetic)
| 18 instructions per call: the stub's lea and bra, then the 16 of lfo_wave_eval.

    .text
    .balign 4
lfo_wave_eval:
    move.l  4(%sp),%d0          | phase
    move.l  %d0,%d1
    lsr.l   #8,%d1
    lsr.l   #8,%d1
    lsr.l   #8,%d1              | phase >> 24
    andi.l  #0xf8,%d1           | (phase >> 27) * 8: the byte offset of the cell
    adda.l  %d1,%a0
    lsr.l   #8,%d0
    lsr.l   #4,%d0              | phase >> 12
    andi.l  #0x7fff,%d0         | f
    move.l  4(%a0),%d1          | d
    muls.l  %d0,%d1             | d * f
    add.l   %d1,%d1             | << 1
    move.l  (%a0),%d0           | v
    add.l   %d1,%d0
    rts

| one stub per added wave k: A0 = the cells of the wave (32 cells of 8 bytes), then the shared evaluation. `lfo_wave_fn` lists their addresses.
    .section .rodata
    .balign 4
    .globl lfo_wave_fn
lfo_wave_fn:
    .text
    .balign 2
    .set    LFO_K, 0
    .rept   LFO_WAVES
    .text
1:  lea     lfo_wave_cells + 256 * LFO_K,%a0
    bra.w   lfo_wave_eval
    .section .rodata
    .long   1b
    .set    LFO_K, LFO_K + 1
    .endr
    .text
    .balign 2

| the interrupt mask of lfo-table.c: raise the level to 7 (the tick runs at 5, the lowest level of the installer's own mask is 7), give back the status register
    .globl lfo_irq_mask
lfo_irq_mask:
    move.w  %sr,%d0
    move.w  #0x2700,%sr
    rts
    .globl lfo_irq_restore
lfo_irq_restore:
    move.l  4(%sp),%d0
    move.w  %d0,%sr
    rts

| the data table of this adapter (the image's info table, name "lfo_table_info"): 'LFOT', version, the first added wave, how many, samples of a shape, set, clear (lfo-table.h)
    .section .rodata
    .balign 4
    .globl lfo_table_info
lfo_table_info:
    .long   0x4c464f54, 1, 7, LFO_WAVES, 32
    .long   lfo_table_set, lfo_table_clear

    .section .bss
    .balign 4
    .globl lfo_wave_cells
lfo_wave_cells:
    .space  256 * LFO_WAVES
