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
