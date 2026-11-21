#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_filter *f = dk_module_ctx_add_filter(ctx, DK_FILTER_ID_RBNK, "Resonator Bank", "RBNK");
    dk_page *p = dk_filter_add_page(f);
    dk_page_add_param(p, "Spread", "SPRD", 64);
    dk_page_add_param(p, "Q", "Q", 20);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
