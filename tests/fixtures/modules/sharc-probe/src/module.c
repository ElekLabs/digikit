/* The effect of test/sharc-probe: one master effect with no knob and no page, declared with a builder call in init.
   The audio passes unchanged (the routine copies its input), so MIX does not matter. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_module_ctx_add_fx(ctx, DK_FX_ID_PROBE, "SHARC probe", "PRB");
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
