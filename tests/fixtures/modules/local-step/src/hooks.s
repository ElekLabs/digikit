| test/local-step: the key hook (both devices, the same code), the entry of the firmware's key walker `bool f(ViewController *vc, KeyEvent *ev)` (ports/fw/input.toml,
| `cf.input.hook.key_event`), as the shortcuts service takes it: it runs in place of the first 8 bytes of the walker (`lea -0x60(sp),sp; movem.l d2-d7/a2-a6,(sp)`) and leaves
| through the tail of the site (the displaced instructions, then the stock walker). The fixture never consumes a key. At the entry 4(sp) = vc and 8(sp) = ev. d0, d1, a0 and a1
| are live at the tail, so the four are saved around the call and restored; the hook never runs in interrupt context (a non-zero interrupt mask goes to the stock walker).

    .text
    .balignw 4, 0x4e71

    .globl local_step_key_hook
local_step_key_hook:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)
    move.w  %sr,%d0
    andi.l  #0x700,%d0
    bne.b   9f
    jsr     dk_local_step_key
9:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     local_step_t_key
