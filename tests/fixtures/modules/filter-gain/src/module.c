/* test/filter-gain: one filter with one own knob (E7). Knob 1 of every module filter is the stock cutoff, so GAIN is knob 2. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_filter *f = dk_module_ctx_add_filter(ctx, DK_FILTER_ID_FGAN, "Filter Gain", "FGAN");
    dk_page *p = dk_filter_add_page(f);
    dk_page_add_param(p, "Gain", "GAIN", 64);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
