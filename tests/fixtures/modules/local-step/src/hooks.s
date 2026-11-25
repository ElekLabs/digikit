| test/local-step: the test hook (both devices, the same code), the site at the end of the SETTINGS list rebuild (ports/fw/menu.toml, `cf.menu.hook.settings_list.test`), as
| `test/svc-test` takes it: it runs in place of the displaced instructions and leaves through the tail of the site. a2 = the settings menu (the C function does not use it).
| d0, d1, a0 and a1 are live at the tail, so the four are saved around the call and restored; the hook never runs in interrupt context (a non-zero interrupt mask goes to the tail).

    .text
    .balignw 4, 0x4e71

    .globl local_step_hook
local_step_hook:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)
    move.w  %sr,%d0
    andi.l  #0x700,%d0
    bne.b   9f
    jsr     dk_local_step_run
9:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     local_step_t_tail
