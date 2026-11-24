| core/rows: `rows_info`, the table that says where the reserved rows are and how a row is made (docs/ARCHITECTURE.md "rows"). One body for every device: the rows-info-<dev>.s files include the device constants,
| then this file.
|
| The image makes room for rows (parameters, machine types, the Digitone II's page rows) and the run-time services fill them. The services read this table (`CoreApi::info`, the name `rows_info`); the platform
| knows the blocks by number only. Every number is the device file's (`[rows]`, `[limits]`, `[modules.knobs]`, through build/gen/device-<dev>.inc) or an address the packer placed (the externs of module.toml); the offsets
| of a row's fields are the field order of the registries in module.toml. All words big endian. A word that is 0 is not there on this device. Layout (the words of `services/rows/src/rows_info.rs`; the tests of
| packages/patchkit/tests/rows.rs read it by these numbers):
|
|   0 'ROWS'   1 version   2 device id
|   3 machines.rows   4 its row size   5 machines.first (the stock type count)   6 machines.count (all rows)   7 the first reserved type   8 the number of reserved types
|   9 mf_names ({long, short, hint} per type)   10 mf_permit   11 mf_pertype (a byte per type; 0: none)
|  12 .. 19 the offsets of a machine row's fields: long, short, f0, clone_of, position, own_dsp, name, short name
|  20 pf_table (the parameter table)   21 params.rows   22 their row size   23 params.first   24 params.count   25 the code of the first reserved parameter (block 1)   26 the rows of block 1
|  27 the row size of the table (60)   28 the offset of `template` in a params.rows row   29 an empty string
|  30 .. 44 the recipe of a parameter row: how each of the 15 words is made (the device file's `[rows] recipe`)
|  45 knobs of a machine   46 the slot of knob 0   47 the template of a reserved parameter row
|  48, 49 not used   50 .. 60 the page rows (0 on a device without): pages.rows, row size, pages.first, the first reserved, reserved rows, then the offsets of name, title, c0, machine, s0, and the `machine`
|     value of a free page row   61 .. 64 not used
|  65 .. 86 the builder profile (the machine facts of the device file)
|  87 machines.entries   88 mf_slotmap (a table of the Digitakt II)   89 the copy of the type reader (Digitone II)   90 mf_list_stock (the stock display order)   91 pages.count
|  92 number of blocks, then 4 words for each block: owner number, first code, rows, template code of a free row   105 reserved parameter rows (the sum of the blocks)
| 106 .. 109 the capacity words the admission reads before an owner loads (`[[info]] cap_<kind>` of module.toml point at them; the kind
|     names are the admission's, the values are the `[rows] caps` keys of `[limits]`)

    .section .rodata
    .balign 4
    .globl rows_info
rows_info:
    .long   0x524f5753                  |  0 'ROWS'
    .long   2                           |  1 version (2: the machine and page rows; 1 had the parameter rows only)
    .long   DEVICE_ID                   |  2 device ([digikit] id)
    .long   _rw_m_rows                  |  3 machines.rows
    .long   _rw_m_row_size              |  4 row size
    .long   _rw_m_first                 |  5 machines.first
    .long   _rw_m_count                 |  6 machines.count
    .long   _rw_m_dyn0                  |  7 the first reserved type
    .long   DYN_TYPES                   |  8 reserved types
    .long   _rw_names                   |  9 {long, short, hint} per type, 12 bytes
    .long   _rw_permit                  | 10 permission long per type
    .long   _rw_pertype                 | 11 one byte per type (0: none)
    .long   0, 4, 8, 44                 | 12 O_LONG, O_SHORT, O_F0 (nine ID fields f0..f8), O_CLONE
    .long   48, 52, 56, 60              | 16 O_POS, O_OWN, O_NAME, O_SNAME
    .long   _rw_table                   | 20 pf_table
    .long   _rw_rows                    | 21 params.rows (name, short, template, ... 7 words)
    .long   _rw_row_size                | 22
    .long   _rw_first                   | 23 params.first
    .long   _rw_count                   | 24 params.count
    .long   _rw_b1                      | 25 the code of the first reserved parameter (block 1)
    .long   ROWS_B1_ROWS                | 26 the rows of block 1
    .long   60                          | 27 parameter table row size
    .long   8                           | 28 offset of `template` in a params.rows row
    .long   mf_empty                    | 29 address of an empty string (machines-<dev>.s)
    .long   ROWS_RECIPE_0, ROWS_RECIPE_1, ROWS_RECIPE_2, ROWS_RECIPE_3, ROWS_RECIPE_4   | 30 the recipe of a row (0..0x3c copies that word of the template's row, 0x100 min, 0x101 max, 0x102 default,
    .long   ROWS_RECIPE_5, ROWS_RECIPE_6, ROWS_RECIPE_7, ROWS_RECIPE_8, ROWS_RECIPE_9   |    0x103 name, 0x104 short, 0x105 to 0x108 computed words)
    .long   ROWS_RECIPE_10, ROWS_RECIPE_11, ROWS_RECIPE_12, ROWS_RECIPE_13, ROWS_RECIPE_14
    .long   PROFILE_KNOBS               | 45 knobs per machine (descriptor fields f0 .. f7) or per page
    .long   PROFILE_SLOT_BASE           | 46 slot of knob 0
    .long   ROWS_B1_TEMPLATE            | 47 the template of a free reserved parameter row
    .long   0                           | 48 (was the machine selection view: it is `machine_view` of core/ui)
    .long   0                           | 49 (was the function that builds its items)
.if ROWS_CAP_2
    .long   _rw_pg_rows                 | 50 page rows (pages.rows)
    .long   _rw_pg_row_size             | 51 page row size
    .long   _rw_pg_first                | 52 pages.first
    .long   _rw_pg_dyn0                 | 53 index of the first reserved page row
    .long   ROWS_CAP_2                  | 54 reserved page rows (the `page_rows` limit)
    .long   0                           | 55 offset of the page name (std::string chars pointer) in a page row
    .long   4                           | 56 title
    .long   8                           | 57 c0 .. c8 (nine parameter codes)
    .long   44                          | 58 machine (the type the page belongs to)
    .long   48                          | 59 s0 .. s8 (nine sound-array slots)
    .long   0x7fffffff                  | 60 the `machine` value of a free page row (no type has it; not -1, which a track without a sound reads)
.else
    .long   0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
.endif
    .long   0, 0, 0, 0                  | 61 .. 64 not used
    .long   PROFILE_CLONE_OF            | 65 clone_of: the machine type every run-time machine clones
    .long   PROFILE_LEVEL               | 66 level code a machine or page carries
    .long   PROFILE_OWN_DSP             | 67 a machine may render with its own DSP routine
    .long   PROFILE_TUNE_CODE           | 68 stock tune code on a machine-row device (0: none)
    .long   PROFILE_TUNE_SLOT           | 69 knob position of the tune code
    .long   PROFILE_LEVEL_CODE          | 70 stock level code on a machine-row device (0: none)
    .long   PROFILE_LEVEL_KNOB          | 71 knob position of the level code
    .long   PROFILE_SLOT_COUNT          | 72 knob positions for words 0.. (count)
    .long   PROFILE_SLOT_0, PROFILE_SLOT_1, PROFILE_SLOT_2, PROFILE_SLOT_3, PROFILE_SLOT_4, PROFILE_SLOT_5, PROFILE_SLOT_6, PROFILE_SLOT_7   | 73 .. 80 positions
    .long   PROFILE_TPL_ENCODER, PROFILE_TPL_FADER, PROFILE_TPL_PAN, PROFILE_TPL_ONOFF, PROFILE_TPL_TUNE, PROFILE_TPL_PLAY   | 81 .. 86 stock rows: encoder, fader, pan, onoff, tune, play
    .long   _rw_m_entries               | 87 machines.entries
    .long   mf_slotmap                  | 88 the descriptor field of mirror index 25 .. 34 (-1: none; machines-dt2.s)
    .long   _rw_reader_real             | 89 the copy of the stock type reader (0: none)
    .long   _rw_list_stock              | 90 the stock display order of the machine list: `machines.first` words, one type each
.if ROWS_CAP_2
    .long   _rw_pg_count                | 91 pages.count
.else
    .long   0
.endif
    .long   3                           | 92 blocks
    .long   ROWS_B1_OWNER, _rw_b1, ROWS_B1_ROWS, ROWS_B1_TEMPLATE  | 93 block 1
    .long   ROWS_B2_OWNER, _rw_b2, ROWS_B2_ROWS, ROWS_B2_TEMPLATE  | 97 block 2
    .long   ROWS_B3_OWNER, _rw_b3, ROWS_B3_ROWS, ROWS_B3_TEMPLATE  | 101 block 3
    .long   ROWS_RESERVED               | 105 reserved parameter rows
    .long   ROWS_CAP_0                  | 106 capacity word 0
    .long   ROWS_CAP_1                  | 107 capacity word 1
    .long   ROWS_CAP_2                  | 108 capacity word 2
    .long   ROWS_CAP_3                  | 109 capacity word 3
