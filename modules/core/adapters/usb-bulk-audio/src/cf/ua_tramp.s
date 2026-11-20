/* core/usb-bulk-audio: ua_init_hook is the hook at the entry of the firmware's USB init (a `hook` op: the displaced prologue,
 * `move.l d2,-(sp); move.l (4,sp),d2`, runs in the tail `ua_init_next`, then the init goes on). It runs ua_prepare with the stack as
 * the init entry left it. d0/d1/a0/a1 are saved; d2-d7/a2-a6 are the C function's to keep. */
    .include "dk_fw_usb.inc"

    .text
    .even
    .globl ua_init_hook
ua_init_hook:
    dk_fw_usb_init_hook ua_prepare, ua_init_next

/* ua_setup_hook is the hook at the entry of the firmware's EP0 setup dispatcher (a `hook` op; no arguments, d0/d1/a0/a1 dead). The USB interrupt calls the dispatcher with
 * `jsr`: if ua_setup_handle answered, `rts` returns to the interrupt as the dispatcher would; otherwise setup_next runs the displaced first instruction and the stock dispatcher. */
    .globl ua_setup_hook
ua_setup_hook:
    jsr     ua_setup_handle
    tst.l   %d0
    beq.s   1f
    rts
1:  jmp     setup_next
