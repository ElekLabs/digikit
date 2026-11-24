| machines.dks, the hooks both devices share: the machine page's draw, code, key and turn entries. Each continues in the stock code through the tail the port file makes
| (`dk_tail_<hash of the hook>`) and leaves as `rts` through `jmp mf_ret_*` (`dk_ret_<hash>`; the aliases are the `[object.externs]` of module.toml), so the routines hold no
| firmware bytes and one routine serves both devices. `DKRT_MF_TAB` is the image table `machines_info` (`info.rs`).

    .text
    .balignw 4, 0x4e71

| ---- the machine page's draw, code, key and turn entries (SourcePageView and MultiSourcePageView, vfunc_4, vfunc_47, vfunc_2, vfunc_17: the same code on both devices; the port file makes the stock tail of each hook for the firmware it binds to). The stock path is the tail the port file makes. At the entry 0(sp) = the return address, 4(sp) = the view, 8(sp) = the event (or canvas); d0/d1/a0/a1 are free.
| The native page seams are Rust functions of this file (`dks/src/native.rs`: `dks_native_draw`, `dks_native_overlay`, `dks_native_code`); -1 from code means pass through; 0 is an intentional unused/LOCAL
| mapping. `dks_page_key` and `dks_page_turn` are Rust (lib.rs).
    .globl mf_draw_hook
mf_draw_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     dks_native_draw
    addq.l  #8,%sp
    tst.l   %d0
    beq.s   1f
    cmpi.l  #2,%d0
    beq.s   2f
    jmp     mf_ret_draw
1:  jmp     dk_tail_04bb3ae8
2:  move.l  8(%sp),-(%sp)               | original stack: return, view, canvas; each callee sees return, view, canvas
    move.l  8(%sp),-(%sp)
    jsr     mf_stock_draw               | the stock function from its first instruction: called, so the stock `rts` returns here
    addq.l  #8,%sp
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     dks_native_overlay
    addq.l  #8,%sp
    jmp     mf_ret_draw

    .globl mf_code_hook
mf_code_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     dks_native_code
    addq.l  #8,%sp
    cmpi.l  #-1,%d0
    beq.s   1f
    jmp     mf_ret_code
1:  jmp     dk_tail_1e264a23

    .globl mf_key_hook
mf_key_hook:
    move.l  8(%sp),-(%sp)       /* event */
    move.l  8(%sp),-(%sp)       /* view: 4(sp) at the entry, 8 after one push */
    jsr     dks_page_key
    addq.l  #8,%sp
    jmp     dk_tail_6113bd45

    .globl mf_turn_hook
mf_turn_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    jsr     dks_page_turn
    addq.l  #8,%sp
    tst.b   %d0
    beq.s   1f
    moveq   #1,%d0                      /* the native dispatcher consumes a nonzero D0b */
    jmp     mf_ret_turn
1:  jmp     dk_tail_d46e3075
