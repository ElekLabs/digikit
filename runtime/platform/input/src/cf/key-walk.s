| core/input: the key walker's hook (ports/fw/input.toml, `cf.input.hook.key_event`): the entry of the firmware's key walker `bool f(ViewController *vc, KeyEvent *ev)`, which every key event of
| the panel passes once in the UI task, before the release callbacks and the views. The hook runs in place of the first 8 bytes of the walker (`lea -0x60(sp),sp; movem.l d2-d7/a2-a6,(sp)`) and
| leaves in one of two ways:
|   consume: `moveq #1,d0; rts`. The stack is as the caller left it; d0 = 1 is the stock "a view took the key" value, which the caller ignores.
|   pass: `jmp input_key_tail`, the tail of the site (the displaced instructions, then the stock walker).
| At the entry 4(sp) = vc and 8(sp) = ev. The decision is `key_walk(vc, ev)` of key_subscribers.c. Strict liveness at the tail (tools.coldfire.liveness): only the flags are dead and
| d0, d1, a0 and a1 are live, so the four are saved around the call and restored on every path; the flags are not restored.
| The walker never runs a subscriber outside task context: the status register is read first, and a non-zero interrupt mask goes to the stock walker.

    .text
    .balignw 4, 0x4e71

    .globl input_key_walk_hook
input_key_walk_hook:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)       | vc is now at 20(sp), ev at 24(sp)
    move.w  %sr,%d0
    andi.l  #0x700,%d0
    bne.b   9f                          | not in task context: the stock walker
    move.l  24(%sp),-(%sp)              | argument 2: ev
    move.l  24(%sp),-(%sp)              | argument 1: vc (the frame is a word deeper)
    jsr     key_walk
    addq.l  #8,%sp
    tst.l   %d0
    beq.b   9f
    movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    moveq   #1,%d0
    rts
9:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     input_key_tail
