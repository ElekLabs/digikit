/* core/platform: the firmware hooks of the digikit runtime and the exception hook install (R3b; moved from core/runtime's tramp.s unchanged).
 * ColdFire, gas syntax, mcpu=54418. The hooks call the runtime's exports by name (dkrt_tick, dkrt_boot, dkrt_midi_rx, dkrt_exc).
 *
 * dkrt_loop_hook replaces the target of the `jsr` at the top of the UI task's event loop (the call that pops the next event
 * from the UI queue; the queue pointer is on the stack, the stock function returns the event in d0). It runs the runtime's
 * tick, then continues in the stock pop as if it had been called directly: the stack is as the caller left it.
 * d0/d1/a0/a1 are the registers the C ABI lets dkrt_tick clobber; the loop has nothing live in them at this call (it
 * has just pushed the argument, and the result comes back in d0). d2-d7/a2-a6 are preserved by the C function.
 * fw_queue_pop is the stock function (anchor cf.rtos.fn.queue_pop).
 */
    .include "device.inc"
    .include "dk_fw_view.inc"
    .include "dk_fw_rtos.inc"

    .text
    .even
    .globl dkrt_loop_hook
dkrt_loop_hook:
    dk_fw_view_ui_loop_hook dkrt_tick, fw_queue_pop


/* dkrt_boot_hook: the hook at the entry of the main-OS task (a `hook` op: the displaced prologue, `link a6,#-0xa8; movem.l ...`,
 * runs in the tail `dkrt_boot_next`, then the task goes on). It runs dkrt_boot (the selection record and the boot-time loads) with
 * the stack as the task entry left it. d0/d1/a0/a1 are saved; d2-d7/a2-a6 are the C function's to keep. */
    .globl dkrt_boot_hook
dkrt_boot_hook:
    dk_fw_rtos_main_os_task_hook dkrt_boot, dkrt_boot_next

/* dkrt_midi_hook: the hook in the MIDI task's dispatch of a gathered USB SysEx message (docs/design/digikit-upload.md). At the hook the
 * stack holds the router's arguments, pushed by the task: 0(sp) = message, 4(sp) = length, 8(sp) = source; the displaced instructions
 * (`movea.l (0,A2,D1*4),A0; jsr (A0)`) call the router. d0/d1/a0/a1 are saved (D1 is live: it indexes the router table). dkrt_midi_rx
 * returns 1 for our messages: continue at dkrt_midi_ret (after the call, where the task pops its arguments and frees the blocks), else
 * run the displaced instructions in dkrt_midi_next. movem and lea leave the condition codes alone. */
    .globl dkrt_midi_hook
dkrt_midi_hook:
    lea     -16(%sp),%sp
    movem.l %d0-%d1/%a0-%a1,(%sp)
    move.l  24(%sp),-(%sp)      /* source */
    move.l  24(%sp),-(%sp)      /* length */
    move.l  24(%sp),-(%sp)      /* message */
    jsr     dkrt_midi_rx
    lea     12(%sp),%sp
    tst.l   %d0
    movem.l (%sp),%d0-%d1/%a0-%a1
    lea     16(%sp),%sp
    bne.s   1f
    jmp     dkrt_midi_next
1:  jmp     dkrt_midi_ret

/* dkrt_exc_hook: the function the firmware's default exception handler calls instead of its own (finding 46). The handler has set sr to 0x2700,
 * saved d0, d1, a0, a1 below the exception frame, and pushed the frame pointer: 0(sp) = return address into the handler, 4(sp) = frame pointer. d2-d7
 * and a2-a6 are still the faulting code's, so they are saved here before any compiled code runs; dkrt_exc(frame, saved) records and resets and
 * does not return. (ColdFire movem has no predecrement form: lea, then movem.) */
    .globl dkrt_exc_hook
dkrt_exc_hook:
    lea     -44(%sp),%sp
    movem.l %d2-%d7/%a2-%a6,(%sp)
    move.l  %sp,-(%sp)          /* saved */
    move.l  52(%sp),-(%sp)      /* frame: 4 (saved) + 44 + 4 (return address) = 52, read before the push */
    jsr     dkrt_exc
1:  bra.s   1b

/* u32 dkplat_exc_install(void): point the firmware's exception hook word at dkrt_exc_hook. `exc_hook_lit` (anchor cf.rtos.site.exc_hook_lit) is the
 * 4-byte literal in the firmware's default exception handler whose value is the address of the hook word; the word is used only when it lies
 * in DDR and is word aligned. Returns 1 when this call changed the word, else 0. The runtime calls it every UI pass: the firmware sets the
 * word in its own boot, so the first call must come after that, and a second setting would be caught by the next pass. */
    .globl dkplat_exc_install
dkplat_exc_install:
    move.l  #exc_hook_lit,%d0
    beq.s   1f                      /* no anchor */
    move.l  %d0,%a0
    move.l  (%a0),%a0               /* the address of the hook word */
    move.l  %a0,%d0
    sub.l   #DDR_CACHED_BASE,%d0
    cmp.l   #DDR_BYTES,%d0
    bhs.s   1f                      /* not in DDR (unsigned) */
    move.l  %a0,%d1
    and.l   #3,%d1
    bne.s   1f                      /* not word aligned */
    move.l  #dkrt_exc_hook,%d1
    cmp.l   (%a0),%d1
    beq.s   1f                      /* already ours */
    move.l  %d1,(%a0)
    moveq   #1,%d0
    rts
1:  moveq   #0,%d0
    rts
