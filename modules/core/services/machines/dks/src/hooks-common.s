| machines.dks, the hooks both devices share: the machine page's draw, code, key and turn entries. Each continues in the stock code through the tail the port file makes
| (`dk_tail_<hash of the hook>`) and leaves as `rts` through `jmp mf_ret_*` (`dk_ret_<hash>`; the aliases are the `[object.externs]` of module.toml), so the routines hold no
| firmware bytes and one routine serves both devices. `MF_TAB` is the image table `machines_info` (`image.rs`).

    .text
    .balignw 4, 0x4e71

    .equ T_N_CODE,    40
    .equ T_N_DRAW,    44
    .equ T_N_OVERLAY, 48

| ---- the machine page's draw, code, key and turn entries (SourcePageView and MultiSourcePageView, vfunc_4, vfunc_47, vfunc_2, vfunc_17: the same code on both devices; the port file makes the stock tail of each hook for the firmware it binds to). The stock path is the tail the port file makes. At the entry 0(sp) = the return address, 4(sp) = the view, 8(sp) = the event (or canvas); d0/d1/a0/a1 are free.
| The native page seams are the adapter's C functions (`mf_native_*` of the image, table words N_*); -1 from code means pass through; 0 is an intentional unused/LOCAL
| mapping. `dks_page_key` and `dks_page_turn` are Rust (lib.rs).
    .globl mf_draw_hook
mf_draw_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    movea.l MF_TAB+T_N_DRAW,%a0
    jsr     (%a0)
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
    movea.l MF_TAB+T_N_OVERLAY,%a0
    jsr     (%a0)
    addq.l  #8,%sp
    jmp     mf_ret_draw

    .globl mf_code_hook
mf_code_hook:
    move.l  8(%sp),-(%sp)
    move.l  8(%sp),-(%sp)
    movea.l MF_TAB+T_N_CODE,%a0
    jsr     (%a0)
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

| ---- run-time filters (docs/plans/extension-model.md E7a). `DKS_FLT` is the table of dkrt-svc-machines `filter::Tab` (the offsets below are checked against it): `F_MASK` bit i is set
| while filter type 16 + i is loaded, `F_BASE` the first request code of the parameter-code function, `F_SLOTS` its answers (3 words per filter), `F_NAMES` the long names and
| `F_ROWS` the descriptor rows (`F_ROW_BYTES` each). A type that is not a loaded module filter takes the stock path, so a stock type behaves as it did.
    .equ F_MASK,      0
    .equ F_BASE,      4
    .equ F_SLOTS,     8
    .equ F_NAMES,     200
    .equ F_ROWS,      264
    .equ F_ROW_BYTES, 44

| The filter type to descriptor row accessor. Entry (6 bytes: `moveq #5,D1; move.l 4(sp),D0`): type at 4(sp); d0, d1 free. A loaded module filter returns its row.
    .globl mf_flt_acc
mf_flt_acc:
    move.l  4(%sp),%d0
    subi.l  #16,%d0
    cmpi.l  #15,%d0
    bhi.s   9f                          | below 16 or above 31 (unsigned: a negative type too)
    move.l  DKS_FLT+F_MASK,%d1
    btst    %d0,%d1
    beq.s   9f
    move.l  #F_ROW_BYTES,%d1
    muls.l  %d1,%d0
    move.l  #DKS_FLT+F_ROWS,%d1
    add.l   %d1,%d0
    jmp     mf_ret_flt_acc
9:  jmp     dk_tail_e1a730f6

| The filter name function: the long name of a loaded module filter. Entry (6 bytes, as the accessor): type at 4(sp); d0, d1 free; a0 is kept.
    .globl mf_flt_name
mf_flt_name:
    move.l  4(%sp),%d0
    subi.l  #16,%d0
    cmpi.l  #15,%d0
    bhi.s   9f
    move.l  DKS_FLT+F_MASK,%d1
    btst    %d0,%d1
    beq.s   9f
    lsl.l   #2,%d0
    move.l  %a0,%d1
    movea.l %d0,%a0
    adda.l  #DKS_FLT+F_NAMES,%a0
    move.l  (%a0),%d0
    movea.l %d1,%a0
    jmp     mf_ret_flt_name
9:  jmp     dk_tail_b13bb53a

| The chooser's order vector: the target of the list builder's call (a0 = the vector object the callee fills). The stock function builds its copy of the six stock types, then the
| routine in Rust appends the loaded module filters. The builder reads neither d0 nor a0 after the call: it loads a3 and d5 from the vector object (DT2 0x40061174 and 0x40061178,
| DN2 0x4005b4e4 and 0x4005b4e8) and d2 (the address of a local) is kept by the C routine.
    .globl mf_flt_list
mf_flt_list:
    move.l  %a0,-(%sp)
    jsr     mf_stock_flt_list
    jsr     dks_filter_list
    addq.l  #4,%sp
    rts
