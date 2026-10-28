| machines/chord on the Digitakt II: the value texts of LOOP, DTUN and INT2-INT4, through core/params' `formatter` (the stock
| formatters' convention: cdecl, `raw` at 4(sp) in 8.8, `buf` at 8(sp); d0/d1/a0/a1 scratch).

    .text
    .globl chord_int_fmt
    .globl chord_bip_fmt
    .globl chord_loop_fmt

| void chord_loop_fmt(int raw, char *buf): (raw >> 8) - 64, OFF at 0 (LOOP: + forward, - ping-pong)
chord_loop_fmt:
    move.l  4(%sp),%d0
    asr.l   #8,%d0
    subi.l  #64,%d0
    movea.l 8(%sp),%a1
    bne.s   ch_num
    lea     ch_off,%a0
1:  move.b  (%a0)+,(%a1)+
    bne.s   1b
    rts

| void chord_int_fmt(int raw, char *buf): step 64 OFF, 65 UNI, below -24 .. -1, above +1 .. +24
chord_int_fmt:
    move.l  4(%sp),%d0
    asr.l   #8,%d0
    subi.l  #64,%d0
    movea.l 8(%sp),%a1
    blt.s   ch_num
    lea     ch_off,%a0
    beq.s   2f
    subq.l  #1,%d0
    lea     ch_uni,%a0
    beq.s   2f
    move.b  #'+',(%a1)+
    bra.s   ch_num
2:  move.b  (%a0)+,(%a1)+
    bne.s   2b
    rts

| void chord_bip_fmt(int raw, char *buf): (raw >> 8) - 64, -64 .. +63
chord_bip_fmt:
    move.l  4(%sp),%d0
    asr.l   #8,%d0
    subi.l  #64,%d0
    movea.l 8(%sp),%a1
ch_num:
    tst.l   %d0
    bge.s   3f
    move.b  #'-',(%a1)+
    neg.l   %d0
3:  cmpi.l  #10,%d0
    blt.s   4f
    divu.w  #10,%d0                 | d0 = remainder << 16 | quotient
    move.l  %d0,%d1
    addi.l  #'0',%d0
    move.b  %d0,(%a1)+
    swap    %d1
    move.l  %d1,%d0
4:  addi.l  #'0',%d0
    move.b  %d0,(%a1)+
    clr.b   (%a1)
    rts

    .section .rodata
ch_off: .ascii "OFF\0"
ch_uni: .ascii "UNI\0"
