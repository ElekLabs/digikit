/* The effect of test/dyn-fx-gain-sendonly: one effect `gain` with one knob, declared with builder calls in init.
   Tab GSO. GAIN is p0, 0 to 1.27: the shown value 0 .. 127 is the gain in percent, so the default 50 is 0.5.
   The MIX knob starts at 127 (wet). The effect starts as a send and can only be a send. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_GAIN, "Test Gain Send Only", "GSO");
    dk_fx_routes(fx, DK_ROUTE_SEND, DK_ROUTE_SEND);
    dk_page *p = dk_fx_add_page(fx, "Test Gain Send Only", "GSO");
    dk_page_add_fx_param(p, "Gain", "GAIN", DK_PERCENT, 0.0f, 1.27f, 0.5f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
