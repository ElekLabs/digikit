| core/card-guard on the Syntakt: the allow-list rows of [card.guard] of devices/syntakt.toml, through build/gen/card-guard-st.inc (`patchkit gen`).
| Three words per row (cg_row of guard.h): first sector, end sector (exclusive), writer class (0 stock, 1 runtime). Unused slots are 0.

    .include "card-guard-st.inc"

    .section .rodata
    .balign 4
    .globl dkcg_row_n
dkcg_row_n:
    .long   CG_N
    .globl dkcg_rows
dkcg_rows:
    .long   CG_FIRST_0, CG_END_0, CG_WHO_0
    .long   CG_FIRST_1, CG_END_1, CG_WHO_1
    .long   CG_FIRST_2, CG_END_2, CG_WHO_2
    .long   CG_FIRST_3, CG_END_3, CG_WHO_3
    .long   CG_FIRST_4, CG_END_4, CG_WHO_4
    .long   CG_FIRST_5, CG_END_5, CG_WHO_5
    .long   CG_FIRST_6, CG_END_6, CG_WHO_6
    .long   CG_FIRST_7, CG_END_7, CG_WHO_7
