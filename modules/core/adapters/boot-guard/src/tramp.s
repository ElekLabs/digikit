    .include "dk_fw_rtos.inc"
    .include "dk_fw_input.inc"
    .text
    .even
    .globl boot_guard_hook
boot_guard_hook:
    dk_fw_rtos_intro_install_hook boot_guard_dispatch, boot_guard_next

    .globl boot_guard_uart_hook
boot_guard_uart_hook:
    dk_fw_input_uart_key_dispatch_hook boot_guard_observe, boot_guard_uart_next

/* The generated wrapper saved the volatile registers and passed the original skip argument.
 * Test the optional link address in assembly: a C function declaration can be optimized as nonnull. */
boot_guard_dispatch:
    /* RAM-only bookkeeping even when skipped/absent; no recovery/runtime/card call. */
    tst.l   BOOT_GUARD_CLOSED
    bne.s   0f
    moveq   #1,%d0
    move.l  %d0,BOOT_GUARD_CLOSED
    move.l  BOOT_GUARD_PANEL_BITS,%d0
    move.l  %d0,BOOT_GUARD_DECISION
0:
    move.l  4(%sp),%d0
    bne.s   1f
    move.l  #dk_boot_early,%d0
    beq.s   1f
    move.l  %d0,-(%sp)
    jsr     boot_guard_enter
    addq.l  #4,%sp
1:  rts
