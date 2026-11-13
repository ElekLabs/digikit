/* The machine of test/dyn-params, declared by builder calls in init: one machine `dynp`. The DSP is test/sine's (`dsp = "sn"`). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_DYNP, "DynP", "DYP");
    /* four parameters on one page; the range differs per word (words 0 .. 3) */
    dk_page *p = dk_machine_add_page(m, "DYNP", "DYP");
    dk_page_add_param(p, "Param 0", "P0", 16);
    dk_param_range(dk_page_add_param(p, "Param 1", "P1", 17), 1, 126);
    dk_param_range(dk_page_add_param(p, "Param 2", "P2", 18), 2, 125);
    dk_param_range(dk_page_add_param(p, "Param 4", "P4", 20), 4, 123);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
