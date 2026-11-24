    .text
    .global peer_picker_move_hook
peer_picker_move_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     peer_picker_move
    addq.l  #8,%sp
    tst.l   %d0
    beq.s   .Ldone
    move.l  %d0,8(%sp)
    jmp     peer_picker_move_tail

    .global peer_picker_confirm_hook
peer_picker_confirm_hook:
    tst.b   11(%sp)
    beq.s   .Lstock_close
    move.l  4(%sp),-(%sp)
    jsr     peer_picker_confirm
    addq.l  #4,%sp
    tst.l   %d0
    beq.s   .Lstock_close
    subq.l  #1,%d0
    bne.s   .Ldone
    clr.l   8(%sp)
.Lstock_close:
    jmp     peer_picker_confirm_tail
.Ldone:
    rts

    /* The stock label callback receives its return string address in A0. */
    .global peer_picker_label
peer_picker_label:
    move.l  %a0,-(%sp)
    jsr     peer_picker_copy_label
    addq.l  #4,%sp
    rts
