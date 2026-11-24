| Host test fixture: the table `machines_info` (version 5, 15 words) (as `DKRT_MF_TAB` of the service file) for the DN2 hooks of core/machines. The firmware exits are not in it: the hooks jump to
| the aliases `mf_x_*`, `mf_ret_*` and compare with `mf_rc<n>` (stubs and symbols of the test unit, `pk_cfunit`); `_mf_*` are symbols the unit gives (registry values).
    .data
    .balign 4
    .globl DKRT_MF_TAB
DKRT_MF_TAB:
    .long   0x4d414348, 5, 2
    .long   _mf_first, _mf_count, _mf_entries, _mf_rows
    .long   mf_ensure
    .long   0, _mf_pgrows, _mf_pgn
    .long   0                      | word 11: the copy of the type reader (unbound here)
    .long   0, 64, 48              | words 12 to 14: the stock list order, the row size, the offset of `position` (unbound here)
