| lfo.dks on the Digitone II (1.12): the six hooks of the LFO tick (vector 191, an interrupt, IPL 5) and of its second reader. Each runs in place of the six stock bytes `moveq 6,Dn; cmp.l D3,Dn; b<cc>` of its site (ports/fw/lfo.toml) and leaves by `jmp` to the stock code.
| The hook runs the displaced moveq and compare itself and takes the stock path at once unless the clamped wave is 6 (`8:`, the stock fall-through or the stock branch target); only on 6 does it continue below, with the stock stack. Its scratch register and the flags are dead on every path from there, proved from the stock bytes (docs/findings/149-lfo-waveforms.md, "Registers"). The clamp turned a raw wave byte of 6 or more into 6,
| so each routine reads the raw byte again: 7 to 14 (`RANGE` of dkrt-svc-lfo: the shape numbers; `services/lfo/tests/route.rs` checks these bounds against the immediates below) is a shape, 6 and 15 to 127 are RAND as stock. The range test is written inline (an interrupt: no call for a stock path).
| `lfo_x_<dev>_*` are the exits of the hooks of ports/fw/lfo.toml (`[object.externs]` of module.toml).
| The routines wait for nothing and call nothing but `dk_lfo_eval`, which reads the shape table and writes nothing but d0, d1, a0, a1 and the flags. The hand-encoded words of the first version are gone: the assembler's
| `mac.l`, `movclr.l` and `muls.l` give the stock bytes (`uv run python -m tools.coldfire.liveness PORT SITE --match HEX` finds them in the stock code).
    .text
    .balignw 4, 0x4e71

| tick_call, DN2 0x40137cc6: `moveq 6,D0; cmp.l D3,D0; beq RAND(0x40137cec)`.
| The hook ran the moveq and the compare itself; its scratch register d0 is dead at both continuations. Here the clamped wave is 6. d2 = phase accumulator,
| the block is a4 (raw wave byte at 0x4c(%a4)). A raw byte of 7 to 14 is a shape: the routine does what the stock path after the beq does for a table wave
| (the phase, the call, the code after the call) with `dk_lfo_eval(phase, wave)` for the function table call. Any other byte (6, or 15 to 127) takes the stock RAND code.
| The DN2 stock call path keeps no register in its frame (d1 is dead at the code after the call), so the routine does not save d1; it saves a0 and a1 (strict liveness does not prove them dead at the exit, finding 149).
| d0, d1, d2 and the flags change; d3 is untouched. RAND: d0 is dead there and d3 is still 6.
    .globl lfo_dn2_tick_call
lfo_dn2_tick_call:
    moveq   #6,%d0                   | the displaced instructions of the site
    cmp.l   %d3,%d0
    bne.b   8f                          | clamped wave is not 6: the stock fall-through
    mvs.b   0x4c(%a4),%d0
    cmpi.l  #7,%d0
    blt.b   1f
    cmpi.l  #14,%d0
    bgt.b   1f
    lea     -8(%sp),%sp                 | save a0 and a1 (live at the exit as far as strict liveness proves): the callee may write them
    movem.l %a0/%a1,(%sp)
    move.l  %d0,-(%sp)                  | the wave (argument 2)
    move.l  #0x636ba875,%d0
    mac.l   %d0,%d2,%acc0               | the phase as the stock code makes it (stock bytes 203c636ba875 a4000800 a1c2 e58a)
    movclr.l %acc0,%d2
    lsl.l   #2,%d2
    move.l  %d2,-(%sp)                  | the phase (argument 1)
    jsr     dk_lfo_eval
    lea     8(%sp),%sp
    movem.l (%sp),%a0/%a1
    lea     8(%sp),%sp
    jmp     lfo_x_dn2_tick_done
1:  jmp     lfo_x_dn2_tick_rand
8:  jmp     lfo_x_dn2_tick_call_end

| tick_slew, DN2 0x40137d1e: `moveq 6,D1; cmp.l D3,D1; bne DONE(0x40137d52)`.
| Here the clamped wave is 6 and the scratch register d1 of the hook is free (dead at the slew code and at DONE). d0 (the function result) is live. A shape has no slew: DONE; any other byte
| runs the slew code of RAND.
    .globl lfo_dn2_tick_slew
lfo_dn2_tick_slew:
    moveq   #6,%d1                   | the displaced instructions of the site
    cmp.l   %d3,%d1
    bne.b   8f                          | clamped wave is not 6: the stock branch to DONE
    mvs.b   0x4c(%a4),%d1
    cmpi.l  #7,%d1
    blt.b   1f
    cmpi.l  #14,%d1
    bgt.b   1f
    jmp     lfo_x_dn2_tick_done
1:  jmp     lfo_x_dn2_tick_slew
8:  jmp     lfo_x_dn2_tick_done

| tick_start, DN2 0x40137c40: `moveq 6,D0; cmp.l D3,D0; beq CLEAR(0x40137c5c)`.
| Here the clamped wave is 6 and the scratch register d2 of the hook is free (dead at the clear and at the SPH phase code). RAND starts at phase 0; a shape starts at the SPH phase like the other waves.
    .globl lfo_dn2_tick_start
lfo_dn2_tick_start:
    moveq   #6,%d0                   | the displaced instructions of the site
    cmp.l   %d3,%d0
    bne.b   8f                          | clamped wave is not 6: the stock fall-through
    mvs.b   0x4c(%a4),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_dn2_tick_start
1:  jmp     lfo_x_dn2_tick_start_rand
8:  jmp     lfo_x_dn2_tick_start

| read_call (the second reader), DN2 0x401378c4: `moveq 6,D0; cmp.l D3,D0; beq RAND(0x401378fa)`.
| The block is a4 (raw wave byte at 0x2a(%a4)), d7 is live. The scratch register d0 of the hook is free. The stock path after the beq makes d0 = 0x32 with a `move.b` onto the moveq's 6, multiplies d2 by it
| (`muls.l d0,d2`) and takes `mac` with 0x7f421f4f; the routine sets d0 itself (`moveq #0x32` is the same value). d1 is live after the call and kept in the stock frame: saved here with a0 and a1.
    .globl lfo_dn2_read_call
lfo_dn2_read_call:
    moveq   #6,%d0                   | the displaced instructions of the site
    cmp.l   %d3,%d0
    bne.b   8f                          | clamped wave is not 6: the stock fall-through
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
    jmp     lfo_x_dn2_read_done
1:  jmp     lfo_x_dn2_read_rand
8:  jmp     lfo_x_dn2_read_call_end

| read_slew, DN2 0x40137932: `moveq 6,D2; cmp.l D3,D2; bne DONE(0x40137962)`. The scratch register d2 of the hook is free (dead at the slew code and at DONE).
    .globl lfo_dn2_read_slew
lfo_dn2_read_slew:
    moveq   #6,%d2                   | the displaced instructions of the site
    cmp.l   %d3,%d2
    bne.b   8f                          | clamped wave is not 6: the stock branch to DONE
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_dn2_read_done
1:  jmp     lfo_x_dn2_read_slew
8:  jmp     lfo_x_dn2_read_done

| read_start, DN2 0x40137836: `moveq 6,D2; cmp.l D3,D2; beq CLEAR(0x40137852)`. The scratch register d2 of the hook is free (dead at the clear and at the SPH phase code).
    .globl lfo_dn2_read_start
lfo_dn2_read_start:
    moveq   #6,%d2                   | the displaced instructions of the site
    cmp.l   %d3,%d2
    bne.b   8f                          | clamped wave is not 6: the stock fall-through
    mvs.b   0x2a(%a4),%d2
    cmpi.l  #7,%d2
    blt.b   1f
    cmpi.l  #14,%d2
    bgt.b   1f
    jmp     lfo_x_dn2_read_start
1:  jmp     lfo_x_dn2_read_start_rand
8:  jmp     lfo_x_dn2_read_start
