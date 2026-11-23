| core/rows on the Digitakt II (1.16, 1.17): `rows_info`, the table of the parameter reservation (docs/ARCHITECTURE.md "rows").
|
| The image makes room for rows and the services fill their own block: the table says where each block is. Every number is
| the device file's `[limits]` (through build/gen/device-dt2.inc) or an address the packer placed (the externs of module.toml).
| All words big endian. Layout (the tests of packages/patchkit/tests/rows.rs read it by these word numbers):
|
|  0 'ROWS'   1 version   2 device id
|  3 the parameter table (pf_table)   4 its row size (60 bytes)
|  5 params.rows (the registry rows: name, short, template, slot, min, max, default)   6 their row size   7 the offset of `template` in one
|  8 params.first (the code of the first added row, the stock count)   9 params.count (all added rows)
| 10 number of blocks, then 4 words for each block: user, first code, rows, template code of a free row
|      user 1: parameters of the run-time machines   2: the knobs of the effect pages   3: the cells of the send page
| 23 reserved rows (the sum of the blocks)
| 24 .. 38 the recipe of a row: how each of the 15 words is made. 0 .. 0x3c = that word of the template's row, 0x100 min, 0x101 max,
|    0x102 default, 0x103 name pointer, 0x104 short pointer, 0x105 mirror index (25 + slot + (slot >> 2) * 2),
|    0x106 kind word ((0x10 + slot ...) << 16 | the template's word 0x18 & 0xffff), 0x107 0x80 + slot, 0x108 the slot itself
| 39 .. 42 the cap words the admission reads before a service loads: machine rows, parameter rows, page rows, send slots
|    (`[[info]] cap_*` of module.toml point at them)

    .include "device-dt2.inc"

    .section .rodata
    .balign 4
    .globl rows_info
rows_info:
    .long   0x524f5753                  |  0 'ROWS'
    .long   1                           |  1 version
    .long   DEVICE_ID                   |  2 device ([digikit] id: 1 = dt2)
    .long   _rw_table                   |  3 pf_table
    .long   60                          |  4 row size of the table
    .long   _rw_rows                    |  5 params.rows
    .long   _rw_row_size                |  6 params.row_size
    .long   8                           |  7 offset of `template` in a params.rows row
    .long   _rw_first                   |  8 params.first
    .long   _rw_count                   |  9 params.count
    .long   3                           | 10 blocks
    .long   1, _rw_dp0, LIMIT_PARAM_ROWS, 0xd1              | 11 parameters of the run-time machines (template: a stock level code)
    .long   2, _rw_fp0, 8 * LIMIT_FX_SLOTS, 0x75            | 15 the knobs of the effect pages (template: Delay "Feedback HPF")
    .long   3, _rw_fs0, LIMIT_SEND_ROWS, 0x75               | 19 the cells of the send page
    .long   LIMIT_PARAM_ROWS + 8 * LIMIT_FX_SLOTS + LIMIT_SEND_ROWS | 23 reserved rows
    .long   0x00, 0x105, 0x100, 0x101, 0x102, 0x14, 0x106, 0x107    | 24 the recipe of a row
    .long   0x20, 0x24, 0x103, 0x2c, 0x104, 0x34, 0x38
    .long   LIMIT_MACHINE_ROWS          | 39 cap_machine_rows
    .long   LIMIT_PARAM_ROWS            | 40 cap_param_rows
    .long   LIMIT_PAGE_ROWS             | 41 cap_page_rows
    .long   LIMIT_SEND_SLOTS            | 42 cap_send_slots
