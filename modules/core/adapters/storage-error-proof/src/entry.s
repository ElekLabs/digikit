/* Entry JMP preserves the caller frame. Pass goes through patchkit's displaced
 * tail; rejection returns -1 directly with arguments still owned by the caller.
 * CCR is caller-saved; C preserves d2-d7/a2-a6. No buffer argument is forwarded. */
    .text
    .even
    .macro entry name, operation, tail
    .globl \name
\name:
    lea     -16(%sp),%sp
    movem.l %d0-%d1/%a0-%a1,(%sp)
    move.l  24(%sp),-(%sp)          /* bytes */
    move.l  24(%sp),-(%sp)          /* sector after first push */
    move.l  #\operation,-(%sp)
    jsr     boot_io_decide
    lea     12(%sp),%sp
    tst.l   %d0
    beq.s   1f
    move.l  #-1,(%sp)              /* replace saved return register */
    movem.l (%sp),%d0-%d1/%a0-%a1
    lea     16(%sp),%sp
    rts
1:  movem.l (%sp),%d0-%d1/%a0-%a1
    lea     16(%sp),%sp
    jmp     \tail
    .endm
    entry boot_io_read, 0, boot_io_read_next
    entry boot_io_write, 1, boot_io_write_next
