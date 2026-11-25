| core/rows on the Digitakt II: the type-row dispatch of a machine type. What else the rows need at run time is the business of the registry's owner.
|
| `mf_dispatch` is the hook at the entry of the firmware's machine descriptor lookup: `FUN_xxxx(type)` returns the descriptor of a type. The rows of the registry are the descriptors of the types from
| `machines.first` on (a row the service has not filled is a gap row); every other type goes to the stock code. A descriptor has the std::string representations of its long and short name: the service writes them
| when it fills a row, and a row without them (a gap row, a row it took back) gets the empty one here. Site: function entry (d0, d1 free); the routine keeps every other register.

    .text
    .equ O_LONG,   0           | descriptor: chars of the std::string long name
    .equ O_SHORT,  4           | descriptor: chars of the std::string short name
    .equ ROWSIZE,  64          | the field order of the registry in module.toml: every field one word

    .globl mf_dispatch
mf_dispatch:
    move.l  4(%sp),%d0
    subi.l  #_mf_first,%d0
    cmpi.l  #_mf_count,%d0
    bcc.s   1f
    move.l  #ROWSIZE,%d1
    muls.l  %d1,%d0
    addi.l  #_mf_rows,%d0
    move.l  %a0,-(%sp)
    movea.l %d0,%a0
    tst.l   O_LONG(%a0)
    bne.s   2f
    move.l  #mf_empty_rep + 12,%d1
    move.l  %d1,O_LONG(%a0)
    move.l  %d1,O_SHORT(%a0)
2:  movea.l (%sp)+,%a0
    rts
1:  jmp     _mf_t_dispatch

    .section .rodata
    .balign 4
    .globl mf_empty_rep
mf_empty_rep:                       | a std::string representation of the empty string: length 0, capacity 0, reference count -1, NUL
    .long   0, 0, -1, 0
    .globl mf_empty
mf_empty:                           | an empty C string (a free row's name)
    .byte   0
    .balign 4

| the descriptor field offset of mirror index 25 .. 34 (-1: none): the slot map of the registry's owner
    .globl mf_slotmap
mf_slotmap:
    .long   8, 12, 16, 20, -1, -1, 24, 28, 32, 36
