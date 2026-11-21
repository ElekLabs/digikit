| Host test fixture: the table `machines_info` (version 4, 21 words) (as `MF_TAB` of the service file) for the DT2 hooks of core/machines. The firmware exits are not in it: the hooks jump to
| the aliases `mf_x_*`, `mf_spt` (stubs and symbols of the test unit, `pk_cfunit`); `_mf_*` are symbols the unit gives (registry values).
    .data
    .balign 4
    .globl MF_TAB
MF_TAB:
    .long   0x4d414348, 4, 1
    .long   _mf_first, _mf_count, _mf_entries, _mf_rows
    .long   0
    .long   mf_native_page_count, 0, 0, 0, 0
    .long   mf_slotmap, 0, 0
    .long   0, 0, 0, 0             | words 16 to 19: the native page calls of the registry (unbound here)
    .long   0                      | word 20: native filter graph (unbound here)

| The service assembles its shared and device hooks in one translation unit.
    .include "tests/fixtures/machines-hooks.s"
    .include "services/machines/dks/src/hooks-dt2.s"
