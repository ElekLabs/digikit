| machines/tb303 on the Digitakt II: the value text of WAVE, through core/params' `formatter` (the stock formatters'
| convention: cdecl, `raw` at 4(sp) in 8.8, `buf` at 8(sp); d0/d1/a0/a1 scratch).
|
|     void tb303_wave_fmt(int raw, char *buf)

    .text
    .globl tb303_wave_fmt
tb303_wave_fmt:
    move.l  4(%sp),%d0
    movea.l 8(%sp),%a1
    lea     tw_saw,%a0
    cmpi.l  #0x80,%d0
    blt.s   1f
    lea     tw_sqr,%a0
1:  move.b  (%a0)+,(%a1)+
    bne.s   1b
    rts

    .section .rodata
tw_saw: .asciz "SAW"
tw_sqr: .asciz "SQR"
