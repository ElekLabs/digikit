| Common service hooks and the boundaries these machine mapping tests never enter.
    .include "services/machines/dks/src/hooks-common.s"

    .text
dk_tail_04bb3ae8:
dk_tail_1e264a23:
dk_tail_6113bd45:
dk_tail_d46e3075:
dk_tail_e1a730f6:
dk_tail_b13bb53a:
dks_filter_graph:
dks_filter_list:
dks_page_key:
dks_page_turn:
mf_ret_code:
mf_ret_draw:
mf_ret_flt_acc:
mf_ret_flt_name:
mf_ret_turn:
mf_stock_draw:
mf_stock_flt_list:
mf_x_flt_list:
mf_x_graph_done:
mf_x_graph_stock:
mf_x_ldr_dn2:
mf_x_ldr_dt2:
    illegal

    .bss
    .balign 4
DKRT_FILTER_TAB:
    .space F_ROWS + 16 * F_ROW_BYTES
