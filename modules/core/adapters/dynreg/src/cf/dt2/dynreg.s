| core/dynreg on the Digitakt II (1.16, 1.17): the table the run-time registry reads, and the machine list filter.
| Design: docs/design/digikit-registry.md. The device id and the number of reserved types are the device file's and
| core/machines types.toml's, through build/gen/device-dt2.inc (`patchkit gen`).
|
| dynreg_info: addresses of the rows and tables core/machines and core/params built at patch time, the row layouts, and
| the lowering of a parameter row (device data: the runtime's registry names no device). All words big endian.
|
| dynreg_copy: replaces the `std::copy` at the end of the machine list getter (`FUN_40052a08`: the static list, sorted once at
| first use, is copied into the vector the caller gets). The copy leaves out the type numbers of reserved rows that no
| module has filled (row name empty). C ABI: copy(begin, end, dest) -> end of the copy in d0; d0, d1, a0, a1 scratch; the
| caller keeps d2-d5/a2/a3 live, so everything else is saved.

    .include "device-dt2.inc"

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
    .long   DEVICE_ID                   |  8 device ([digikit] id: 1 = dt2)
    .long   _dr_rows                    | 12 machines.rows
    .long   _dr_row_size                | 16 row size
    .long   _dr_first                   | 20 machines.first
    .long   _dr_count                   | 24 machines.count
    .long   _dr_dyn0                    | 28 first reserved type
    .long   DYN_TYPES                   | 32 reserved types
    .long   _dr_names                   | 36 {long, short, hint} per type, 12 bytes
    .long   _dr_permit                  | 40 permission long per type
    .long   _dr_pertype                 | 44 one byte per type (0: none)
    .long   0                           | 48 O_LONG   (offsets in a machines.rows row)
    .long   4                           | 52 O_SHORT
    .long   8                           | 56 O_F0 (nine ID fields f0..f8)
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
    | 120: how each of the 15 words of a parameter row is made: 0..0x3c = copy that word of the template's row,
    | 0x100 min, 0x101 max, 0x102 default, 0x103 name pointer, 0x104 short pointer, 0x105 mirror index
    | (25 + slot + (slot >> 2) * 2), 0x106 kind word (see core/params module.toml), 0x107 0x80 + slot
    .long   0x00, 0x105, 0x100, 0x101, 0x102, 0x14, 0x106, 0x107
    .long   0x20, 0x24, 0x103, 0x2c, 0x104, 0x34, 0x38
    .long   8                           | 180 knobs per machine (descriptor fields f0 .. f7)
    .long   0                           | 184 slot of knob 0
    .long   0xd1                        | 188 the template of a free reserved parameter row
    .long   0                           | 192 (was the machine-view variable: archived with hot reload, docs/plans/digikit-hot-reload.md)
    .long   0                           | 196 (was the view builder)
    .long   0                           | 200 page rows (pages.rows; 0: the device keeps its knobs in the machine row)
    .long   0                           | 204 page row size
    .long   0                           | 208 pages.first
    .long   0                           | 212 index of the first reserved page row
    .long   0                           | 216 reserved page rows
    .long   0                           | 220 offset of the page name (std::string chars pointer) in a page row
    .long   0                           | 224 title
    .long   0                           | 228 c0 .. c8 (nine parameter codes)
    .long   0                           | 232 machine (the type the page belongs to)
    .long   0                           | 236 s0 .. s8 (nine sound-array slots)
    .long   0                           | 240 the `machine` value of a free page row (no type has it)
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
