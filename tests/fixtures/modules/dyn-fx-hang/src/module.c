/* The effect of test/dyn-fx-hang: one effect `hang` with one knob (unused by the routine), declared with builder calls in init. Starts as a send. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_HANG, "Test FX Hang", "HNG");
    dk_fx_routes(fx, DK_ROUTE_SEND | DK_ROUTE_MASTER, DK_ROUTE_SEND);
    dk_page *p = dk_fx_add_page(fx, "Test FX Hang", "HNG");
    dk_page_add_fx_param(p, "Gain", "GAIN", DK_PERCENT, 0.0f, 1.27f, 0.5f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
