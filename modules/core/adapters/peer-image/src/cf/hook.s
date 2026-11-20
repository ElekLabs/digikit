/* core/peer-image: the hook and the wrapper entry (docs/findings/151-syntakt-section7-pack.md). ColdFire, gas syntax, mcpu=54418.
 *
 * peer_hook replaces the thread's `lea <NOR read>.l,A2` (a `hook` op, nothing displaced): A2 gets peer_read and the thread goes on after the site.
 * The thread calls `jsr (a2)` twice, with the stack [return][src][len][dst] and the caller popping the arguments. peer_read keeps that signature:
 * it copies the three arguments for the C function peer_load (the C code keeps D2-D7 and A2-A6, which the thread has live), and when peer_load
 * reports a failed check it takes the stock path of a missing section 7: it pops the return address and the three arguments and jumps to the
 * code that stores status 1 (peer_status_fail), where the thread waits and MAIN shows its boot failure screen. */
    .text
    .even
    .globl peer_hook
peer_hook:
    lea     peer_read,%a2
    jmp     peer_ret

    .globl peer_read
peer_read:
    move.l  12(%sp),-(%sp)        /* dst (the stack is [ret][src][len][dst]; each copy moves the next argument to 12(sp)) */
    move.l  12(%sp),-(%sp)        /* len */
    move.l  12(%sp),-(%sp)        /* src */
    jsr     peer_load
    lea     12(%sp),%sp
    tst.l   %d0
    bne.s   1f
    rts
1:  lea     16(%sp),%sp           /* the return address and the three arguments */
    jmp     peer_status_fail
