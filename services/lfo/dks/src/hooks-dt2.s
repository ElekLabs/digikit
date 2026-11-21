| lfo.dks on the Digitakt II (1.17): the routines the six pointer slots of core/lfo run (an interrupt: the LFO tick, vector 191, and its second reader).
| Each image stub ran the stock `moveq 6,Dn; cmp.l D3,Dn; b<cc>` and enters here only when the wave clamped to 0..6 is 6, with the stock stack; its scratch register (the one the slot test
| overwrote) and the flags are dead on every path from here, proved from the stock bytes (docs/findings/149-lfo-waveforms.md, "Registers"). The clamp turned a raw wave byte of 6 or more into 6,
| so each routine reads the raw byte again: 7 to 14 (the numbers of the shapes: `RANGE` of dkrt-svc-lfo) is a shape, 6 and 15 to 127 are RAND as stock. Every path leaves by `jmp` to the stock code:
| `lfo_x_*` are the exits of the hooks of ports/fw/lfo.toml (`[object.externs]` of module.toml).
| The routines wait for nothing and call nothing but `dk_lfo_eval`, which reads the shape table and writes nothing but d0, d1, a0, a1 and the flags. The hand-encoded words of the first version are gone: the assembler's
| `mac.l`, `movclr.l` and `muls.l` give the stock bytes (`uv run python -m tools.coldfire.liveness PORT SITE --match HEX` finds them in the stock code).
    .text
    .balignw 4, 0x4e71

| tick_call, DT2 0x40139804: `moveq 6,D7; cmp.l D3,D7; beq RAND(0x40139832)`.
| The image stub ran the moveq and the compare, and the stub's slot test left d0 free (dead at both continuations). Here the clamped wave is 6. d2 = phase accumulator,
| the block is a3 (raw wave byte at 2(%a3)). A raw byte of 7 to 14 is a shape: the routine does what the stock path after the beq does for a table wave
| (the phase, the call, the code after the call) with `dk_lfo_eval(phase, wave)` for the function table call. Any other byte (6, or 15 to 127) takes the stock RAND code.
| The stock call path keeps d1 in its frame (live after the call), so the routine saves it, and a0 and a1 with it.
| d0, d2 and the flags change as they do on the stock call path; d1, a0 and a1 are saved and restored (strict liveness does not prove a0 and a1 dead at the exit, finding 149); d3 is untouched.
| RAND: d0 is dead there and d3 is still 6.
    .globl lfo_dt2_tick_call
lfo_dt2_tick_call:
    mvs.b   2(%a3),%d0
    cmpi.l  #7,%d0
    blt.b   1f
    cmpi.l  #14,%d0
    bgt.b   1f
    lea     -12(%sp),%sp                | save d1, a0 and a1 (live at the exit as far as strict liveness proves): the callee may write a0 and a1, and d1 is live
    movem.l %d1/%a0/%a1,(%sp)
    move.l  %d0,-(%sp)                  | the wave (argument 2)
    move.l  #0x636ba875,%d0
    mac.l   %d0,%d2,%acc0               | the phase as the stock code makes it (stock bytes 203c636ba875 a4000800 a1c2 e58a)
    movclr.l %acc0,%d2
    lsl.l   #2,%d2
    move.l  %d2,-(%sp)                  | the phase (argument 1)
    jsr     dk_lfo_eval
    lea     8(%sp),%sp
    movem.l (%sp),%d1/%a0/%a1
    lea     12(%sp),%sp
    jmp     lfo_x_tick_done
1:  jmp     lfo_x_tick_rand

| tick_slew, DT2 0x4013986c: `moveq 6,D2; cmp.l D3,D2; bne DONE(0x4013989e)`.
| Here the clamped wave is 6 and the stub left d2 free (dead at the slew code and at DONE). d0 (the function result) is live. A shape has no slew: DONE; any other byte
| runs the slew code of RAND.
    .globl lfo_dt2_tick_slew
lfo_dt2_tick_slew:
    mvs.b   2(%a3),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_tick_done
1:  jmp     lfo_x_tick_slew

| tick_start, DT2 0x4013977e: `moveq 6,D0; cmp.l D3,D0; beq CLEAR(0x4013979a)`.
| Here the clamped wave is 6 and the stub left d2 free (dead at the clear and at the SPH phase code). RAND starts at phase 0; a shape starts at the SPH phase like the other waves.
    .globl lfo_dt2_tick_start
lfo_dt2_tick_start:
    mvs.b   2(%a3),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_tick_start
1:  jmp     lfo_x_tick_start_rand

| read_call (the second reader), DT2 0x401394dc: `moveq 6,D0; cmp.l D3,D0; beq RAND(0x40139512)`.
| The block is a4 (raw wave byte at 0x2a(%a4)), d7 is live. The stub left d0 free. The stock path after the beq makes d0 = 0x32 with a `move.b` onto the moveq's 6, multiplies d2 by it
| (`muls.l d0,d2`) and takes `mac` with 0x7f421f4f; the routine sets d0 itself (`moveq #0x32` is the same value). d1 is live after the call and kept in the stock frame: saved here with a0 and a1.
    .globl lfo_dt2_read_call
lfo_dt2_read_call:
    mvs.b   0x2a(%a4),%d0
    cmpi.l  #7,%d0
    blt.b   1f
    cmpi.l  #14,%d0
    bgt.b   1f
    lea     -12(%sp),%sp
    movem.l %d1/%a0/%a1,(%sp)
    move.l  %d0,-(%sp)
    moveq   #0x32,%d0
    muls.l  %d0,%d2                     | stock bytes 4c002800
    move.l  #0x7f421f4f,%d0
    mac.l   %d0,%d2,%acc0               | stock bytes 203c7f421f4f a4000800 a1c2 e58a
    movclr.l %acc0,%d2
    lsl.l   #2,%d2
    move.l  %d2,-(%sp)
    jsr     dk_lfo_eval
    lea     8(%sp),%sp
    movem.l (%sp),%d1/%a0/%a1
    lea     12(%sp),%sp
    jmp     lfo_x_read_done
1:  jmp     lfo_x_read_rand

| read_slew, DT2 0x4013954a: `moveq 6,D2; cmp.l D3,D2; bne DONE(0x4013957a)`. The stub left d2 free (dead at the slew code and at DONE).
    .globl lfo_dt2_read_slew
lfo_dt2_read_slew:
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_read_done
1:  jmp     lfo_x_read_slew

| read_start, DT2 0x4013944e: `moveq 6,D2; cmp.l D3,D2; beq CLEAR(0x4013946a)`. The stub left d2 free (dead at the clear and at the SPH phase code).
    .globl lfo_dt2_read_start
lfo_dt2_read_start:
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_read_start
1:  jmp     lfo_x_read_start_rand
