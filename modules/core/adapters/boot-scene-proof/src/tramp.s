    .text
    .even
    .globl boot_scene_hook
boot_scene_hook:
    move.l  %d0,-(%sp)
    move.l  BOOT_SCENE_PUBLISHED,%d0
    beq.s   1f
    move.l  %d0,8(%sp)
1:  move.l  (%sp)+,%d0
    jmp     boot_scene_next

/* Optional address must be checked in assembly, before calling any C preparation. */
    .globl dk_boot_early
dk_boot_early:
    move.l  #dk_boot_asset,%d0
    beq.s   2f
    move.l  #dk_boot_asset_discard,-(%sp)
    move.l  %d0,-(%sp)
    jsr     boot_scene_prepare
    addq.l  #8,%sp
2:  rts
