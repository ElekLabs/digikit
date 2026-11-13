/* The effect of test/trace-demo: one effect `gain` with one knob (as test/dyn-fx-gain), declared with builder calls in init, and trace marks in init. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include <dk/core/trace.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_GAIN, "Test Gain", "GAN");
    dk_page *p = dk_fx_add_page(fx, "Test Gain", "GAN");
    dk_page_add_fx_param(p, "Gain", "GAIN", DK_PERCENT, 0.0f, 1.27f, 0.5f);
    DK_TRACE_LOG("trace-demo init", 1);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
