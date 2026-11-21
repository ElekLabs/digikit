| shortcuts.dks, the key hook (both devices, the same code): the entry of the firmware's key walker `bool f(ViewController *vc, KeyEvent *ev)`, which every key event of the panel passes once
| in the UI task, before the release callbacks and the views (ports/fw/input.toml, `cf.input.hook.key_event`). The hook runs in place of the first 8 bytes of the walker
| (`lea -0x60(sp),sp; movem.l d2-d7/a2-a6,(sp)`) and leaves in one of two ways:
|   consume: `moveq #1,d0; rts`. The stack is as the caller left it, so nothing of the displaced code has to be undone; d0 = 1 is the stock "a view took the key" value, which the caller ignores.
|   pass: `jmp shortcuts_t_key`, the tail of the site (the displaced instructions, then the stock walker).
| At the entry 4(sp) = vc and 8(sp) = ev (+0xc key code, +0x10 flags: bit 0 set on a press). The decision is `dk_shortcuts_key(code, flags)` of hooks.rs (the crate's `Shortcuts::on_key`); it
| only decides and queues the module's `on` and `off`: no module code and no view runs inside the walker (the UI pass makes the calls, `Service::tick`). Registers (strict liveness, tools.coldfire.liveness on the three firmwares): at the tail only the flags are dead and d0, d1, a0 and a1 are live, so the four are saved around the call and restored
| on every path; the flags are not restored. The hook never runs in interrupt context: the status register is read first, and a non-zero interrupt mask goes to the stock walker.

    .text
    .balignw 4, 0x4e71

    .globl shortcuts_key_hook
shortcuts_key_hook:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)       | vc is now at 20(sp), ev at 24(sp)
    move.w  %sr,%d0
    andi.l  #0x700,%d0
    bne.b   9f                          | not in task context: the stock walker
    movea.l 24(%sp),%a0
    move.l  0x10(%a0),-(%sp)            | argument 2: the flags
    move.l  0xc(%a0),-(%sp)             | argument 1: the key code
    jsr     dk_shortcuts_key
    addq.l  #8,%sp
    tst.l   %d0
    beq.b   9f
    movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    moveq   #1,%d0
    rts
9:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     shortcuts_t_key
