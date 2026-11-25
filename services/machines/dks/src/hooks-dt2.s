| machines.dks on the Digitakt II (1.17): the ColdFire routines the service's hooks run. The dispatch is the image's (`runtime/platform/rows/src/cf/machines-dt2.s`).
|
| Each hook is entered by a `jmp.l` that the runtime writes at its firmware site (a hook of a service is raw: no trampoline). The routine takes the whole
| control flow of the site and ends in the stock code either by `jmp dk_tail_<hash of the hook>` (the displaced instructions of the site as in the user's firmware, made by
| the port file) or, when a branch is among them, by `jmp mf_x_*` to an exit that the hook declares in `ports/fw` (`exits`; `dk_exit_<hash>_<n>`, an absolute address the port file resolves:
| the jump changes no register or flag). The aliases `mf_x_*`, `mf_spt` are the `[object.externs]` of module.toml.
| `DKRT_MF_TAB` holds the registry words of the image's `rows_info` (core/rows), bound by `start` (`info.rs`); its words give the registry rows and counts and the address of the slot map. Offsets T_* below; the same layout is in hooks-dn2.s and in `image.rs`.
|
| Hook convention: registers the stock site frees are scratch (`ports/fw/machine.toml` lists them per hook); everything else a routine saves.
| The image keeps the dispatch hook (`mf_dispatch` of core/rows: it answers a type of the registry with its row).

    .text
    .balignw 4, 0x4e71

| row layout (bytes), the field order of core/rows' [[registry.field]]
    .equ O_CLONE,  44
    .equ ROWSIZE,  64


| the table (byte offsets of the words of DKRT_MF_TAB; `image.rs` has the same layout)
    .equ T_FIRST, 12
    .equ T_COUNT, 16
    .equ T_ROWS, 24
    .equ T_SLOTMAP, 28

| mfd_row: d0 = index k (< count) -> a0 = &machines.rows[k]. Preserves every other register (not the flags).
mfd_row:
    move.l  %d1,-(%sp)
    move.l  #ROWSIZE,%d1
    muls.l  %d0,%d1
    add.l   DKRT_MF_TAB+T_ROWS,%d1
    movea.l %d1,%a0
    move.l  (%sp)+,%d1
    rts

| clone.5 is the masked test `(type & ~2) == 4` (types 4 and 6): d0 becomes the clone_of of an added type before the stock mask. Site (12 bytes): the test itself,
| `moveq #-3,D1; and.l D1,D0; moveq #4,D1; cmp.l D0,D1; beq.w T`. It continues at T (the table word EQ) when equal, else behind the site.
    .globl mf_clone5
mf_clone5:
    move.l  %a0,-(%sp)
    move.l  %d0,%d1
    sub.l   DKRT_MF_TAB+T_FIRST,%d0
    cmp.l   DKRT_MF_TAB+T_COUNT,%d0
    bcc.s   1f
    bsr.w   mfd_row
    move.l  O_CLONE(%a0),%d1
1:  move.l  %d1,%d0
    movea.l (%sp)+,%a0
    moveq   #-3,%d1                     | the stock test
    and.l   %d1,%d0
    moveq   #4,%d1
    cmp.l   %d0,%d1
    bne.s   2f
    jmp     mf_x_c5_eq
2:  jmp     mf_x_c5_ne

| ---- type argument of the parameter mapping: `FUN(index, type, filter)` answers the parameter code of a (mirror index 25..34, machine type) pair. For an added type:
| the slots with a descriptor field answer with the entry's field, the two without (29, 30: MANUAL SLICE's Sample Bank / Slot rows, which the stock map holds for its
| owner but no descriptor lists) answer as the clone does (the stack argument is replaced by the clone's type, then the stock code runs); every other index runs the
| stock code. Site (8 bytes): `move.l 0x10(sp),D0; movea.l 0x14(sp),A1` in the middle of the function; a0 = index, d1 live; d0, a1 are reloaded, d2 and a2 are saved by
| the function (they are saved here too).
    .globl mf_typearg
mf_typearg:
    move.l  0x10(%sp),%d0
    sub.l   DKRT_MF_TAB+T_FIRST,%d0
    cmp.l   DKRT_MF_TAB+T_COUNT,%d0
    bcc.s   9f
    lea     -16(%sp),%sp
    movem.l %d1-%d2/%a0/%a2,(%sp)
    move.l  %a0,%d2
    subi.l  #25,%d2
    cmpi.l  #9,%d2
    bhi.s   7f
    lsl.l   #2,%d2
    movea.l DKRT_MF_TAB+T_SLOTMAP,%a2
    move.l  (%a2,%d2.l),%d2
    bmi.s   6f
    bsr.w   mfd_row
    move.l  (%a0,%d2.l),%d0
    movem.l (%sp),%d1-%d2/%a0/%a2
    lea     16(%sp),%sp
    jmp     mf_x_typearg                | the second exit of the stock code, with d0 = the field
6:  bsr.w   mfd_row
    move.l  O_CLONE(%a0),%d0
    movem.l (%sp),%d1-%d2/%a0/%a2
    lea     16(%sp),%sp
    move.l  %d0,0x10(%sp)
    bra.s   9f
7:  movem.l (%sp),%d1-%d2/%a0/%a2
    lea     16(%sp),%sp
9:  jmp     dk_tail_09514a38            | the stock instructions of the site, copied by the runtime from the port file, and on

| ---- SRC page count (docs/findings/50). The page list of a `ParameterPageView` is a vector of 4-byte entries at view+0x7c (begin) and view+0x80 (end); the current page is
| view+0x90. `SourcePageView` has two entries (the knob page and the sample view). An added machine has ONE SRC page: at the start of `SourcePageView::vfunc_4` the vector is cut
| to SRC_PAGES entries and the page index is 0 when the track's type is an added one, and a stock type gets its two entries back. Site (8 bytes, the first test of the
| function): `tst.l 0x90(A2); beq.w T`; a2 = the view, d2 = the draw context. T (table word SP_ALT) is the knob-page branch; the stock path goes on at site + 8
| (SP_STOCK). SPT is `FUN_4005f6bc` (the type as a signed long, -1 without a track). d0, d1, a0, a1 are free at the site.
    .equ    SRC_PAGES,  1
    .equ    SRC_STOCK,  2           | entries of the stock vector
    .globl mf_srcpage
mf_srcpage:
    move.l  %a2,-(%sp)
    jsr     mf_spt
    addq.l  #4,%sp
    cmp.l   DKRT_MF_TAB+T_FIRST,%d0
    blt.s   1f
    move.l  #SRC_PAGES,-(%sp)
    move.l  %d0,-(%sp)
    jsr     dks_native_page_count
    addq.l  #8,%sp
    move.l  %d0,%d1
    lsl.l   #2,%d1
    add.l   0x7c(%a2),%d1
    move.l  %d1,0x80(%a2)            | existing capacity is two entries; no allocation or vector growth
    cmp.l   0x90(%a2),%d0
    bhi.s   4f
    clr.l   0x90(%a2)
4:  jmp     mf_x_sp_alt
1:  move.l  0x80(%a2),%d0           | any other type: the stock vector again if an added type cut it
    sub.l   0x7c(%a2),%d0
    subq.l  #4 * SRC_PAGES,%d0
    bne.s   2f
    move.l  0x7c(%a2),%d1
    addi.l  #4 * SRC_STOCK,%d1
    move.l  %d1,0x80(%a2)
2:  tst.l   0x90(%a2)
    beq.w   3f
    jmp     mf_x_sp_stock
3:  jmp     mf_x_sp_alt

| ---- rank (the sort comparator's std::map insert): it gets the (type, display position) pairs of DKRT_MF_RANK, which `start` built (src/list.rs), instead of the caller's array.
| Site: the `jsr` of the insert; stack: 0 map, 4 begin, 8 end. d0 free. The pairs are 8 bytes; DKRT_MF_RANKN of them.
    .globl mf_rank
mf_rank:
    move.l  #DKRT_MF_RANK,%d0
    move.l  %d0,4(%sp)
    move.l  DKRT_MF_RANKN,%d0
    lsl.l   #3,%d0
    addi.l  #DKRT_MF_RANK,%d0
    move.l  %d0,8(%sp)
    jmp     mf_x_rank
