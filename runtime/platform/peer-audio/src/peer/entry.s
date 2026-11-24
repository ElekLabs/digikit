    /* Section 7 of Syntakt OS 1.42, checked by core/peer-image before extension. */
    .equ STOCK_IMAGE,          0x40000400
    .equ STOCK_ENTRY,          0x40001070
    .equ STOCK_STACK,          0x48000000
    .equ STOCK_STACK_OPERAND,  0x4000107c
    .equ STOCK_RENDER,         0x4000168e
    .equ STOCK_RENDER_OPERAND, 0x40000abe

    .equ RECEIVER,             0x47fdf000
    .equ RECEIVER_STATE,       0x47fdfc00
    .equ PCM_BANKS,            0x47fe0000
    .equ PEER_MAILBOX,         0x600
    .equ MAIN_MAILBOX,         0x10000000 + PEER_MAILBOX

    .section .text.peer_audio,"ax"
    .balign 4
    .globl peer_audio_entry, peer_audio_render
peer_audio_entry:
    lea     -16(%sp),%sp
    movem.l %d0-%d1/%a0-%a1,(%sp)
    lea     peer_audio_render(%pc),%a0
    lea     peer_audio_end(%pc),%a1
    move.l  %a1,%d0
    sub.l   %a0,%d0
    lsr.l   #2,%d0
    movea.l #RECEIVER,%a1
1:  move.l  (%a0)+,(%a1)+
    subq.l  #1,%d0
    bne.s   1b
    movea.l #RECEIVER_STATE,%a1
    move.l  #((PCM_BANKS-RECEIVER_STATE)/4),%d1
2:  clr.l   (%a1)+
    subq.l  #1,%d1
    bne.s   2b
    movem.l (%sp),%d0-%d1/%a0-%a1
    lea     16(%sp),%sp
    jmp     STOCK_ENTRY

    .p2alignw 2,0x4e71
peer_audio_render:
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    move.l  20(%sp),-(%sp)
    jsr     STOCK_RENDER
    lea     20(%sp),%sp
    lea     -16(%sp),%sp
    movem.l %d0-%d1/%a0-%a1,(%sp)
    pea     PCM_BANKS
    move.l  24(%sp),-(%sp)
    pea     RECEIVER_STATE
    pea     PEER_MAILBOX
    bsr.w   peer_audio_frame
    lea     16(%sp),%sp
    movem.l (%sp),%d0-%d1/%a0-%a1
    lea     16(%sp),%sp
    rts

    .section .rodata
    .balign 4
    .globl peer_audio_layout, peer_audio_mailbox
peer_audio_layout:
    .long STOCK_IMAGE,STOCK_ENTRY,STOCK_STACK,STOCK_STACK_OPERAND
    .long STOCK_RENDER,STOCK_RENDER_OPERAND,RECEIVER,RECEIVER_STATE
peer_audio_mailbox:
    .long MAIN_MAILBOX
