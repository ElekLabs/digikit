| core/polynotes on the Digitakt II 1.17: audio tracks whose machine has a poly_notes row record up to four notes per trig.
|
|   pn_rec    replaces the note writers' calls of is_audio(track set, track): the stock answer, except 0 for a poly track
|   pn_trig   vector_191_handler, a new trig on an audio track after its sound and locks are sent: on a poly track, notes 2-4 of
|             the trig's note list (each 0x40 + its interval from note 1, 0 none) go into the frame long at +0xf4 + 0x60 t,
|             halfwords {note3 << 8 | note2, note4}; the DSP's frame copy has them as word 61: note2 | note3 << 8 | note4 << 16.
|             Stock writes that long only for type 6 (MANUAL SLICE's slice table).
|
| Rows of the poly_notes registry: type.

    .equ R_TYPE, 0
    .equ TYPE, 0xa2             | the machine type byte of a Sound
    .equ SOUND_OFF, 0x34        | track t's Sound in the live kit
    .equ SOUND_STRIDE, 0x450
    .equ EV_POOL, 0x1c          | the trig event: the note pool's node array
    .equ EV_HEAD, 0x20          | its first node (-1: none)
    .equ EV_LOCAL, -0x64        | the handler's local that holds the event
    .equ NODE_NOTE, 2
    .equ NEXT, 0xd000           | the s16 next node of each node, from the node array
    .equ FRAME_NOTES, 0xf4
    .equ FRAME_STRIDE, 0x60

    .text

| d0 = 1 when track d0's machine has a row, else 0; clobbers d1, a0, a1
    .globl pn_poly
pn_poly:
    cmpi.l  #16,%d0
    bcc.s   8f
    movea.l pn_kit_lit,%a0
    move.l  (%a0),%d1
    beq.s   8f
    movea.l %d1,%a0
    move.l  #SOUND_STRIDE,%d1
    mulu.l  %d1,%d0
    adda.l  %d0,%a0
    moveq   #0,%d0
    move.b  SOUND_OFF+TYPE(%a0),%d0
    lea     pn_rows,%a1
    move.l  #pn_count,%d1
1:  tst.l   %d1
    beq.s   8f
    cmp.l   R_TYPE(%a1),%d0
    beq.s   9f
    adda.l  #pn_rowsize,%a1
    subq.l  #1,%d1
    bra.s   1b
8:  moveq   #0,%d0
    rts
9:  moveq   #1,%d0
    rts

    .globl pn_rec
pn_rec:                         | 4(sp) the track set, 8(sp) the track
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     pn_isaudio
    addq.l  #8,%sp
    tst.b   %d0
    beq.s   9f
    move.l  8(%sp),%d0
    bsr.s   pn_poly
    eori.l  #1,%d0
9:  rts

    .globl pn_trig
pn_trig:                        | a6 the handler's frame, d2 the track; every register it uses is put back
    lea     -24(%sp),%sp
    movem.l %d0-%d3/%a0-%a1,(%sp)
    move.l  %d2,%d0
    bsr.w   pn_poly
    tst.l   %d0
    beq.s   9f
    movea.l EV_LOCAL(%a6),%a0
    movea.l EV_POOL(%a0),%a1
    move.l  EV_HEAD(%a0),%d0
    moveq   #0,%d3              | 0 for each note there is not
    moveq   #3,%d1
1:  tst.l   %d0
    bmi.s   3f
    movea.l %a1,%a0
    adda.l  #NEXT,%a0
    add.l   %d0,%d0
    move.w  (%a0,%d0.l),%d0
    ext.l   %d0
    bmi.s   3f
    movea.l %d0,%a0
    adda.l  %d0,%a0
    adda.l  %d0,%a0             | 3 x the node
    adda.l  %a0,%a0             | 6 x
    moveq   #0,%d2
    move.b  NODE_NOTE(%a1,%a0.l),%d2
    cmpi.l  #0x80,%d2
    bcs.s   2f
    moveq   #0,%d2              | 0xff: none
2:  lsl.l   #8,%d3
    move.b  %d2,%d3
    subq.l  #1,%d1
    bne.s   1b
    bra.s   4f
3:  lsl.l   #8,%d3              | the notes past the end of the list
    subq.l  #1,%d1
    bne.s   3b
4:  move.l  %d3,%d0             | 0x00aabbcc -> 0xbb00aacc: the DSP's word is note2 | note3 << 8 | note4 << 16
    andi.l  #0xff00,%d0
    swap    %d0
    andi.l  #0xff00ff,%d3
    or.l    %d0,%d3
    move.l  8(%sp),%d2          | the track
    move.l  #FRAME_STRIDE,%d0
    mulu.l  %d2,%d0
    movea.l pn_tx_lit,%a0
    adda.l  %d0,%a0
    move.l  %d3,FRAME_NOTES(%a0)
9:  movem.l (%sp),%d0-%d3/%a0-%a1
    lea     24(%sp),%sp
    jmp     pn_trig_tail
