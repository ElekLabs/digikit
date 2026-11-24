    /* Syntakt 1.42 section 7, guarded by core/peer-image's full section digest. */
    .equ PEER_IMAGE_BASE,     0x40000400
    .equ PEER_STOCK_ENTRY,   0x40001070
    .equ PEER_STOCK_STACK,   0x48000000
    .equ PEER_STACK_OPERAND, 0x4000107c
    .equ PEER_STOCK_RENDER,  0x4000168e
    .equ PEER_RENDER_OPERAND,0x40000abe

    /* The proof removes these two pages from the top of the stock stack. */
    .equ PEER_SAMPLE_RAM,    0x47ffe000
    .equ PEER_SAMPLE_TRIGGER,0x600
    .equ PEER_SAMPLE_MAGIC,  0x44535031

    .text
    .balign 4
    .globl peer_sample_blob, peer_sample_entry, peer_sample_frame, peer_sample_state
peer_sample_blob:
peer_sample_entry:
    lea     -12(%sp),%sp
    movem.l %d0/%a0-%a1,(%sp)
    lea     peer_sample_frame(%pc),%a0
    movea.l #PEER_SAMPLE_RAM,%a1
    move.l  peer_sample_state(%pc),%d0
    add.l   %d0,%d0
    addi.l  #(peer_sample_state-peer_sample_frame+8+3),%d0
    lsr.l   #2,%d0
1:  move.l  (%a0)+,(%a1)+
    subq.l  #1,%d0
    bne.s   1b
    movem.l (%sp),%d0/%a0-%a1
    lea     12(%sp),%sp
    jmp     PEER_STOCK_ENTRY

peer_sample_frame:
    /* Preserve the stock call's five arguments and the return state. */
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    jsr     PEER_STOCK_RENDER
    lea     20(%sp),%sp
    lea     -28(%sp),%sp
    movem.l %d0-%d4/%a0-%a1,(%sp)
    lea     (PEER_SAMPLE_RAM+(peer_sample_state-peer_sample_frame)),%a0
    move.l  4(%a0),%d0
    move.l  PEER_SAMPLE_TRIGGER,%d1
    cmpi.l  #PEER_SAMPLE_MAGIC,%d1
    bne.s   2f
    clr.l   PEER_SAMPLE_TRIGGER
    clr.l   %d0
2:  move.l  (%a0),%d3
    cmp.l   %d3,%d0
    bhs.s   6f
    movea.l 32(%sp),%a1
    lea     8(%a0,%d0.l*2),%a0
    moveq   #31,%d2
3:  move.w  (%a0)+,%d1
    swap    %d1
    clr.w   %d1
    move.l  (%a1),%d4
    add.l   %d1,%d4
    bvc.s   5f
    tst.l   %d1
    bmi.s   4f
    move.l  #0x7fffffff,%d4
    bra.s   5f
4:  move.l  #0x80000000,%d4
5:  move.l  %d4,(%a1)+
    addq.l  #1,%d0
    cmp.l   %d3,%d0
    bhs.s   6f
    subq.l  #1,%d2
    bpl.s   3b
6:  move.l  %d0,(PEER_SAMPLE_RAM+(peer_sample_state-peer_sample_frame)+4)
    movem.l (%sp),%d0-%d4/%a0-%a1
    lea     28(%sp),%sp
    rts

    .balign 4
peer_sample_state:
    /* MAIN appends the two state words and the aligned PCM here. */

    .section .rodata
    .balign 4
    .globl peer_sample_layout
peer_sample_layout:
    .long   PEER_IMAGE_BASE,PEER_STOCK_ENTRY,PEER_STOCK_STACK,PEER_STACK_OPERAND
    .long   PEER_STOCK_RENDER,PEER_RENDER_OPERAND,PEER_SAMPLE_RAM
