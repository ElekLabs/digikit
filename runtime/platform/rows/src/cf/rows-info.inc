| core/rows: `rows_info`, the table of the parameter reservation (docs/ARCHITECTURE.md "rows"). One body for every device: the rows-info-<dev>.s files include the device constants, then this file.
|
| The image makes room for rows and the loaded services fill their blocks. The platform knows the blocks by number only: every number is
| the device file's `[rows]` and `[limits]` (through build/gen/device-<dev>.inc) or an address the packer placed (the externs of module.toml).
| All words big endian. Layout (the tests of packages/patchkit/tests/rows.rs read it by these word numbers):
|
|  0 'ROWS'   1 version   2 device id
|  3 the parameter table (pf_table)   4 its row size (60 bytes)
|  5 params.rows (the registry rows: name, short, template, slot, min, max, default)   6 their row size   7 the offset of `template` in one
|  8 params.first (the code of the first added row, the stock count)   9 params.count (all added rows)
| 10 number of blocks, then 4 words for each block: owner number, first code, rows, template code of a free row
| 23 reserved rows (the sum of the blocks)
| 24 .. 38 the recipe of a row: how each of the 15 words is made (the device file's `[rows] recipe`)
| 39 .. 42 the capacity words the admission reads before a service loads (`[[info]] cap_<kind>` of module.toml point at them; the kind
|    names are the admission's, the values are the `[rows] caps` keys of `[limits]`)

    .section .rodata
    .balign 4
    .globl rows_info
rows_info:
    .long   0x524f5753                  |  0 'ROWS'
    .long   1                           |  1 version
    .long   DEVICE_ID                   |  2 device ([digikit] id)
    .long   _rw_table                   |  3 pf_table
    .long   60                          |  4 row size of the table
    .long   _rw_rows                    |  5 params.rows
    .long   _rw_row_size                |  6 params.row_size
    .long   8                           |  7 offset of `template` in a params.rows row
    .long   _rw_first                   |  8 params.first
    .long   _rw_count                   |  9 params.count
    .long   3                           | 10 blocks
    .long   ROWS_B1_OWNER, _rw_b1, ROWS_B1_ROWS, ROWS_B1_TEMPLATE  | 11 block 1
    .long   ROWS_B2_OWNER, _rw_b2, ROWS_B2_ROWS, ROWS_B2_TEMPLATE  | 15 block 2
    .long   ROWS_B3_OWNER, _rw_b3, ROWS_B3_ROWS, ROWS_B3_TEMPLATE  | 19 block 3
    .long   ROWS_RESERVED               | 23 reserved rows
    .long   ROWS_RECIPE_0, ROWS_RECIPE_1, ROWS_RECIPE_2, ROWS_RECIPE_3, ROWS_RECIPE_4  | 24 the recipe of a row
    .long   ROWS_RECIPE_5, ROWS_RECIPE_6, ROWS_RECIPE_7, ROWS_RECIPE_8, ROWS_RECIPE_9
    .long   ROWS_RECIPE_10, ROWS_RECIPE_11, ROWS_RECIPE_12, ROWS_RECIPE_13, ROWS_RECIPE_14
    .long   ROWS_CAP_0                  | 39 capacity word 0
    .long   ROWS_CAP_1                  | 40 capacity word 1
    .long   ROWS_CAP_2                  | 41 capacity word 2
    .long   ROWS_CAP_3                  | 42 capacity word 3
