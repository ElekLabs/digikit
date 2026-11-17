| lfo.dks on the Digitone II (1.12): the routines the six pointer slots of core/lfo run (an interrupt: the LFO tick, vector 191, and its second reader). Each routine takes
| the place of the six stock bytes `moveq 6,Dn; cmp.l D3,Dn; beq|bne` that compare the wave clamped to 0..6 with 6 (RAND). The image stub of the slot enters here with a1 = this
| address. A wave byte of 7 or more was clamped to 6; the routine reads the unclamped byte again and, for a value above 6, calls `dk_lfo_eval(phase, wave)` (signed Q31) in
| place of the stock function table call, with the stock code's own contract (d0, d1, a0 and a1 are free after a call; the stock saves d1 around the call, so the routine does).
| Every other wave leaves by `jmp` to the stock code: `lfo_x_*` are the exits of the hooks of devices/fw/lfo.toml (`[object.externs]` of module.toml).
| The routines wait for nothing and call nothing but `dk_lfo_eval`, which reads the shape table and writes nothing.
    .text
    .balignw 4, 0x4e71

| tick_call: site `moveq 6,D0; cmp.l D3,D0; beq RAND`. d2 = phase accumulator, d3 = clamped wave, a4 = block (wave byte at 0x4c(a4)). exit: the table call; exit.1: RAND; exit.2: the code after the call.
    .globl lfo_dn2_tick_call
lfo_dn2_tick_call:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x4c(%a4),%d0
    cmpi.l  #6,%d0
    ble.w   2f
    move.l  %d1,-(%sp)
    move.l  %d0,-(%sp)                  | the wave (argument 2)
    move.l  #0x636ba875,%d0             | the phase, as the stock code makes it: mac.l d0,d2,acc0; movclr.l acc0,d2; lsl.l #2,d2
    .word   0xa400,0x0800
    .word   0xa1c2
    lsl.l   #2,%d2
    move.l  %d2,-(%sp)                  | the phase (argument 1)
    jsr     dk_lfo_eval
    lea     8(%sp),%sp
    move.l  (%sp)+,%d1
    jmp     lfo_x_tick_done
1:  jmp     lfo_x_tick_call
2:  jmp     lfo_x_tick_rand

| tick_slew: site `moveq 6,D2; cmp.l D3,D2; bne DONE`. d0 and d1 are live; d2 is free. exit: the slew code of RAND; exit.1: skip it.
    .globl lfo_dn2_tick_slew
lfo_dn2_tick_slew:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x4c(%a4),%d2
    cmpi.l  #6,%d2
    bgt.w   1f
    jmp     lfo_x_tick_slew
1:  jmp     lfo_x_tick_done

| tick_start: site `moveq 6,D0; cmp.l D3,D0; beq CLEAR`. RAND starts at phase 0; a table shape starts at the SPH phase like the other waves. exit: the SPH phase; exit.1: the clear.
    .globl lfo_dn2_tick_start
lfo_dn2_tick_start:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x4c(%a4),%d0
    cmpi.l  #6,%d0
    ble.w   2f
1:  jmp     lfo_x_tick_start
2:  jmp     lfo_x_tick_start_rand

| read_call: the second reader. a4 = block (wave byte at 0x2a(a4)), d7 is live. Its phase is `muls.l #0x32; mac 0x7f421f4f; lsl.l #2`.
    .globl lfo_dn2_read_call
lfo_dn2_read_call:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x2a(%a4),%d0
    cmpi.l  #6,%d0
    ble.w   2f
    move.l  %d1,-(%sp)
    move.l  %d0,-(%sp)
    moveq   #0x32,%d0
    muls.l  %d0,%d2
    move.l  #0x7f421f4f,%d0
    .word   0xa400,0x0800
    .word   0xa1c2
    lsl.l   #2,%d2
    move.l  %d2,-(%sp)
    jsr     dk_lfo_eval
    lea     8(%sp),%sp
    move.l  (%sp)+,%d1
    jmp     lfo_x_read_done
1:  jmp     lfo_x_read_call
2:  jmp     lfo_x_read_rand

    .globl lfo_dn2_read_slew
lfo_dn2_read_slew:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #6,%d2
    bgt.w   1f
    jmp     lfo_x_read_slew
1:  jmp     lfo_x_read_done

    .globl lfo_dn2_read_start
lfo_dn2_read_start:
    cmpi.l  #6,%d3
    bne.w   1f
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #6,%d2
    ble.w   2f
1:  jmp     lfo_x_read_start
2:  jmp     lfo_x_read_start_rand
