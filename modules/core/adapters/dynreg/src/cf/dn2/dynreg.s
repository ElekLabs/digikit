| core/dynreg on the Digitone II (1.11, 1.12): the table the run-time registry reads, and the machine list filter.
| Design: docs/design/digikit-registry.md. The device id and the number of reserved types are the device file's and
| core/machines types.toml's, through build/gen/device-dn2.inc (`patchkit gen`).
|
| dynreg_info: addresses of the rows and tables core/machines and core/params built at patch time, the row layouts, and
| the lowering of a parameter row (device data: the runtime's registry names no device). All words big endian.
|
| dynreg_copy: the `std::copy` at the end of the machine list getter (`FUN_4004d8b6`: the static list is copied into the
| vector the caller gets; the getter calls the copy through `jsr (A3)`, so dynreg_list_hook below replaces the call and the
| store after it). The copy leaves out the type numbers of reserved rows that no
| module has filled (row name empty). C ABI: copy(begin, end, dest) -> end of the copy in d0; d0, d1, a0, a1 scratch; the
| caller keeps d2-d5/a2/a3 live, so everything else is saved.

    .include "device-dn2.inc"

    .text

    .equ O_NAME, 56             | machines.s: pointer to the display name (C string); empty = the row is free

    .globl dynreg_copy
dynreg_copy:
    lea     -12(%sp),%sp
    movem.l %d2-%d3/%a2,(%sp)           | args now at 16 (begin), 20 (end), 24 (dest)
    movea.l 16(%sp),%a0
    move.l  20(%sp),%d1
    movea.l 24(%sp),%a1
1:  cmpa.l  %d1,%a0
    bcc.s   9f
    move.l  (%a0)+,%d0
    move.l  %d0,%d2
    subi.l  #_dr_dyn0,%d2
    cmpi.l  #DYN_TYPES,%d2
    bcc.s   2f                          | not a reserved type: keep
    addi.l  #_dr_dyn0,%d2               | the row index is the type - machines.first
    subi.l  #_dr_first,%d2
    move.l  #_dr_row_size,%d3
    muls.l  %d2,%d3
    addi.l  #_dr_rows,%d3
    movea.l %d3,%a2
    movea.l O_NAME(%a2),%a2             | the name pointer
    tst.b   (%a2)
    beq.s   1b                          | an empty name: the row is free, leave the type out
2:  move.l  %d0,(%a1)+
    bra.s   1b
9:  move.l  %a1,%d0
    movem.l (%sp),%d2-%d3/%a2
    lea     12(%sp),%sp
    rts

| dynreg_list_hook: the six bytes at the end of the getter, `jsr (A3); move.l D0,(4,A2)` (A3 = the std::copy of the library),
| replaced by a jump here. The three arguments are on the stack as at the original call; a2 is the vector being returned.
    .globl dynreg_list_hook
dynreg_list_hook:
    jsr     dynreg_copy
    move.l  %d0,4(%a2)
    jmp     _dr_t_copy + 6

    .section .rodata
    .balign 4
    .globl dynreg_empty
dynreg_empty:
    .byte 0
    .balign 4

    .globl dynreg_info
dynreg_info:
    .long   0x44594e52                  |  0 'DYNR'
    .long   3                           |  4 version (3: builder profile from 260; 2: page, unload from 200)
    .long   DEVICE_ID                   |  8 device ([digikit] id: 2 = dn2)
    .long   _dr_rows                    | 12 machines.rows
    .long   _dr_row_size                | 16 row size
    .long   _dr_first                   | 20 machines.first
    .long   _dr_count                   | 24 machines.count
    .long   _dr_dyn0                    | 28 first reserved type
    .long   DYN_TYPES                   | 32 reserved types
    .long   _dr_names                   | 36 {long, short, hint} per type, 12 bytes
    .long   _dr_permit                  | 40 permission long per type
    .long   0                           | 44 one byte per type (0: the Digitone II has none)
    .long   0                           | 48 O_LONG   (offsets in a machines.rows row)
    .long   4                           | 52 O_SHORT
    .long   8                           | 56 O_F0 (nine ID fields f0..f8; unused on the Digitone II, the page rows carry the knobs)
    .long   44                          | 60 O_CLONE
    .long   48                          | 64 O_POS
    .long   52                          | 68 O_OWN
    .long   56                          | 72 O_NAME
    .long   60                          | 76 O_SNAME
    .long   _dr_p_table                 | 80 the parameter table (pf_table, rows of 60 bytes)
    .long   _dr_p_rows                  | 84 params.rows (name, short, template, ... 7 words)
    .long   _dr_p_row_size              | 88
    .long   _dr_p_first                 | 92 params.first
    .long   _dr_p_count                 | 96 params.count
    .long   _dr_p_dyn0                  | 100 code of the first reserved parameter
    .long   16                          | 104 reserved parameters
    .long   60                          | 108 parameter table row size
    .long   8                           | 112 offset of `template` in a params.rows row
    .long   dynreg_empty                | 116 address of an empty string
    | 120: how each of the 15 words of a parameter row is made (see the Digitakt II table): the Digitone II copies words
    | 0x18 and 0x1c of the template and takes the sound-array slot itself (0x108) as the mirror index
    .long   0x00, 0x108, 0x100, 0x101, 0x102, 0x14, 0x18, 0x1c
    .long   0x20, 0x24, 0x103, 0x2c, 0x104, 0x34, 0x38
    .long   8                           | 180 knobs per page
    .long   25                          | 184 sound-array slot of knob 0
    .long   0xd5                        | 188 the template of a free reserved parameter row (A Env Decay)
    .long   0                           | 192 (was the machine-view variable: archived with hot reload, docs/plans/digikit-hot-reload.md)
    .long   0                           | 196 (was the view builder)
    .long   _dr_pg_rows                 | 200 page rows (pages.rows)
    .long   _dr_pg_row_size             | 204 page row size
    .long   _dr_pg_first                | 208 pages.first
    .long   _dr_pg_dyn0                 | 212 index of the first reserved page row
    .long   8                           | 216 reserved page rows
    .long   0                           | 220 offset of the page name (std::string chars pointer) in a page row
    .long   4                           | 224 title
    .long   8                           | 228 c0 .. c8 (nine parameter codes)
    .long   44                          | 232 machine (the type the page belongs to)
    .long   48                          | 236 s0 .. s8 (nine sound-array slots)
    .long   0x7fffffff                  | 240 the `machine` value of a free page row (no type has it; not -1, which a track without a sound reads)
    .long   0                           | 244 (was project(); the in-use scan is archived)
    .long   0                           | 248 (was kit_of)
    .long   0                           | 252 (was track_type)
    .long   0                           | 256 (was the track count)
    | 260: the builder profile (core/runtime commit of a module that declares through builder calls): the device facts the packer
    | took from the device file's [modules] and [adapter.machine] before builders (clone_of, level default, knob positions of
    | a machine's words, stock rows by glyph). Generated from devices/*.toml (`patchkit gen`, build/gen/device-{dev}.inc).
    .long   PROFILE_CLONE_OF   | 260 clone_of: the machine type every run-time machine clones
    .long   PROFILE_LEVEL      | 264 level code a machine or page carries
    .long   PROFILE_OWN_DSP    | 268 a machine may render with its own DSP routine
    .long   PROFILE_TUNE_CODE  | 272 stock tune code on a machine-row device (0: none)
    .long   PROFILE_TUNE_SLOT  | 276 knob position of the tune code
    .long   PROFILE_LEVEL_CODE | 280 stock level code on a machine-row device (0: none)
    .long   PROFILE_LEVEL_KNOB | 284 knob position of the level code
    .long   PROFILE_SLOT_COUNT | 288 knob positions for words 0.. (count)
    .long   PROFILE_SLOT_0, PROFILE_SLOT_1, PROFILE_SLOT_2, PROFILE_SLOT_3, PROFILE_SLOT_4, PROFILE_SLOT_5, PROFILE_SLOT_6, PROFILE_SLOT_7   | 292 .. 320 positions
    .long   PROFILE_TPL_ENCODER, PROFILE_TPL_FADER, PROFILE_TPL_PAN, PROFILE_TPL_ONOFF, PROFILE_TPL_TUNE, PROFILE_TPL_PLAY   | 324 .. 344 stock rows: encoder, fader, pan, onoff, tune, play
