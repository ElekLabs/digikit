| test/svc-test: the raw hook of the service (devices/fw/menu.toml, cf.menu.hook.settings_list.test). It runs in place of the displaced instructions of the site at the end of the
| SETTINGS list rebuild: d0, d1, a0 and a1 are saved, the C function counts the run (a2 = the settings menu), and the tail runs the displaced instructions and leaves behind the site.
    .text
    .balignw 4, 0x4e71
    .globl svc_test_hook
svc_test_hook:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)
    move.l  %a2,-(%sp)
    jsr     svc_test_settings_tail
    addq.l  #4,%sp
    movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     svc_test_t_tail
