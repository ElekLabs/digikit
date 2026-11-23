| Host test fixture: the table `machines_info` (version 3, 20 words) (as `DKRT_MF_TAB` of the service file) for the DT2 hooks of core/machines. The firmware exits are not in it: the hooks jump to
| the aliases `mf_x_*`, `mf_spt` (stubs and symbols of the test unit, `pk_cfunit`); `_mf_*` are symbols the unit gives (registry values).
    .data
    .balign 4
    .globl DKRT_MF_TAB
DKRT_MF_TAB:
    .long   0x4d414348, 3, 1
    .long   _mf_first, _mf_count, _mf_entries, _mf_rows
    .long   0
    .long   mf_native_page_count, 0, 0, 0, 0
    .long   mf_slotmap, 0, 0
    .long   0, 0, 0, 0             | words 16 to 19: the native page calls of the registry (unbound here)
