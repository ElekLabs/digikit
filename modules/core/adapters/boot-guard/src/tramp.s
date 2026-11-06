    .include "dk_fw_rtos.inc"
    .text
    .even
    .globl boot_guard_hook
boot_guard_hook:
    dk_fw_rtos_intro_install_hook boot_guard_dispatch, boot_guard_next

/* The generated wrapper saved the volatile registers and passed the original skip argument.
 * Test the optional link address in assembly: a C function declaration can be optimized as nonnull. */
boot_guard_dispatch:
    move.l  4(%sp),%d0
    bne.s   1f
    move.l  #dk_boot_early,%d0
    beq.s   1f
    move.l  %d0,-(%sp)
    jsr     boot_guard_enter
    addq.l  #4,%sp
1:  rts
