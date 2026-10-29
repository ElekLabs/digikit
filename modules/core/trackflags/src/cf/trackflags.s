| core/trackflags on the Digitakt II 1.17: sixteen flag bits per track that a machine toggles with FUNC + a knob press, kept with the
| sound and sent to the DSP. The bits are the halfword Sound[0] (`Sound + 0x14`): a runtime index no parameter uses, which the stock
| sound saver already writes to record slot 0 (and stock firmware loads into nothing).
|
|   tf_key    the SRC page's key handler (MachineParameterPageView::vfunc_2, entry): a knob press with FUNC held, on a track whose
|             machine has a `flag_toggles` row for that knob, flips the row's bit, shows "<label> ON" / "<label> OFF" (the bit set is
|             OFF, so 0 keeps a machine's default) and takes the event; everything else runs the stock handler
|   tf_frame  vector_191_handler, after its per-track loop: for each track whose machine has a row, Sound[0] goes into the frame word
|             at +0xf8 (the last of the six words the stock code writes only for type 6, the slice boundaries); the DSP reads it there
|   tf_load   the sound loader, after its table loop (which leaves the padding slots in index 0): index 0 = record slot 0
|
| Rows of the flag_toggles registry (module.toml field order): type, knob (0..7), mask, label.

    .equ R_TYPE, 0
    .equ R_KNOB, 4
    .equ R_MASK, 8
    .equ R_LABEL, 12
    .equ FLAGS, 0x14            | Sound[0]
    .equ TYPE, 0xa2             | the machine type byte of a Sound
    .equ KEY_FUNC, 0x11
    .equ KEY_KNOB_A, 0x29
    .equ FRAME_FLAGS, 0xf8
    .equ FRAME_STRIDE, 0x60
    .equ SOUND_OFF, 0x34        | track t's Sound in the live kit (devices/digitakt-ii.toml [fx] sound_off, sound_stride)
    .equ SOUND_STRIDE, 0x450

    .text

| a0 = the row for machine type d0 and knob d1 (knob -1: any), or 0; clobbers d2, a1
tf_find:
    lea     tf_rows,%a1
    move.l  #tf_count,%d2
1:  tst.l   %d2
    beq.s   3f
    cmp.l   R_TYPE(%a1),%d0
    bne.s   2f
    tst.l   %d1
    bmi.s   4f
    cmp.l   R_KNOB(%a1),%d1
    beq.s   4f
2:  adda.l  #tf_rowsize,%a1
    subq.l  #1,%d2
    bra.s   1b
3:  suba.l  %a1,%a1
4:  movea.l %a1,%a0
    rts

    .globl tf_key
tf_key:                         | 4(sp) the view, 8(sp) the event
    movea.l 8(%sp),%a0
    move.l  0xc(%a0),%d0
    subi.l  #KEY_KNOB_A,%d0
    cmpi.l  #8,%d0
    bcc.w   9f                  | not a knob
    move.l  0x10(%a0),%d1
    btst    #0,%d1
    beq.w   9f
    btst    #3,%d1
    bne.w   9f                  | not a press
    lea     -20(%sp),%sp
    movem.l %d2-%d4/%a2-%a3,(%sp)
    move.l  %d0,%d3             | the knob
    pea     KEY_FUNC
    jsr     tf_held
    addq.l  #4,%sp
    tst.l   %d0
    beq.w   8f
    movea.l 24(%sp),%a2         | the view
    move.l  0x74(%a2),-(%sp)
    jsr     tf_track            | the active track
    addq.l  #4,%sp
    cmpi.l  #16,%d0
    bcc.w   8f
    movea.l tf_kit_lit,%a0
    move.l  (%a0),%d1           | the live kit
    beq.w   8f
    move.l  #SOUND_STRIDE,%d2
    mulu.l  %d2,%d0
    add.l   %d1,%d0
    addi.l  #SOUND_OFF,%d0
    movea.l %d0,%a3             | its Sound
    moveq   #0,%d0
    move.b  TYPE(%a3),%d0
    move.l  %d3,%d1
    bsr.w   tf_find
    tst.l   %a0
    beq.w   8f
    move.l  R_MASK(%a0),%d1
    move.w  FLAGS(%a3),%d0
    eor.l   %d1,%d0
    move.w  %d0,FLAGS(%a3)
    lea     tf_on,%a1
    and.l   %d1,%d0
    beq.s   5f
    lea     tf_off,%a1
5:  move.l  %a1,-(%sp)
    move.l  R_LABEL(%a0),-(%sp)
    pea     tf_fmt
    move.l  0x6c(%a2),-(%sp)
    jsr     tf_msg
    lea     16(%sp),%sp
    movem.l (%sp),%d2-%d4/%a2-%a3
    lea     20(%sp),%sp
    moveq   #1,%d0
    rts
8:  movem.l (%sp),%d2-%d4/%a2-%a3
    lea     20(%sp),%sp
9:  jmp     tf_key_tail

    .globl tf_frame
tf_frame:                       | inside the interrupt handler: every register it uses is put back
    lea     -36(%sp),%sp
    movem.l %d0-%d4/%a0-%a3,(%sp)
    movea.l tf_tx_lit,%a2       | the frame being built
    movea.l tf_kit_lit,%a3
    move.l  (%a3),%d0           | the live kit
    beq.s   3f
    movea.l %d0,%a3
    lea     SOUND_OFF(%a3),%a3
    moveq   #16,%d3
1:  movea.l %a3,%a0
    move.w  FLAGS(%a0),%d4
    moveq   #0,%d0
    move.b  TYPE(%a0),%d0
    moveq   #-1,%d1
    bsr.w   tf_find
    tst.l   %a0
    beq.s   2f
    move.w  %d4,FRAME_FLAGS(%a2)
2:  lea     FRAME_STRIDE(%a2),%a2
    lea     SOUND_STRIDE(%a3),%a3
    subq.l  #1,%d3
    bne.s   1b
3:  movem.l (%sp),%d0-%d4/%a0-%a3
    lea     36(%sp),%sp
    jmp     tf_frame_tail

    .globl tf_load
tf_load:                        | a2 the Sound, a3 its record
    move.w  0x1c(%a3),FLAGS(%a2)
    jmp     tf_load_tail

    .section .rodata
tf_fmt: .asciz "%s %s"
tf_on:  .asciz "ON"
tf_off: .asciz "OFF"
