#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_ASCEND, "Ascend", "ASC");
    dk_fx_routes(fx, DK_ROUTE_SEND, DK_ROUTE_SEND);
    dk_page *p = dk_fx_add_page(fx, "Ascend", "ASC");
    dk_page_add_fx_param(p, "Sweep Rate", "RATE", DK_PERCENT, 0.04f, 0.30f, 0.10f);
    dk_page_add_fx_param(p, "Sweep Direction", "DIR", DK_BIPOLAR, -64.0f, 63.0f, 63.0f);
    dk_page_add_fx_param(p, "Notch Density", "DENS", DK_PERCENT, 0.0f, 1.0f, 0.5f);
    dk_page_add_fx_param(p, "Notch Depth", "DEPTH", DK_PERCENT, 0.0f, 1.0f, 0.45f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
