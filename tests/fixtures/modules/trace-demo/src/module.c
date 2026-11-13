/* The effect page of test/trace-demo: one effect `gain` with one knob (as test/dyn-fx-gain), and trace marks in init. */
#include <dk/core/module.h>
#include <dk/fx/fx.h>
#include <dk/core/trace.h>

DK_FX(gain, .process = "_dg_process", .desc = "_dg_desc", .mix = 127);
DK_FX_PARAM(gain, 0, "Gain", "GAIN", DK_PERCENT, 0, 1.27, 0.5);
DK_PAGE(gain, "GAN", "Test Gain", DK_KNOB_AT(0, 0));

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    DK_TRACE_LOG("trace-demo init", 1);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
