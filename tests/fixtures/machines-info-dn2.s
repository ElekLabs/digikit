| Host test fixture: the table `machines_info` (version 4, 21 words) (as `MF_TAB` of the service file) for the DN2 hooks of core/machines. The firmware exits are not in it: the hooks jump to
| the aliases `mf_x_*`, `mf_ret_*` and compare with `mf_rc<n>` (stubs and symbols of the test unit, `pk_cfunit`); `_mf_*` are symbols the unit gives (registry values).
    .data
    .balign 4
    .globl MF_TAB
MF_TAB:
    .long   0x4d414348, 4, 2
    .long   _mf_first, _mf_count, _mf_entries, _mf_rows
    .long   mf_ensure
    .long   mf_native_page_count, mf_native_descriptor, 0, 0, 0
    .long   0, _mf_pgrows, _mf_pgn
    .long   0, 0, 0, 0             | words 16 to 19: the native page calls of the registry (unbound here)
    .long   0                      | word 20: native filter graph (unbound here)
