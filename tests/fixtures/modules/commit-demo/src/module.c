/* test/commit-demo: a machine with three params and an effect, declared with builder calls in init (docs/plans/sdk.md 6.2).
   The DSP symbols are _cmach_render, _cmach_note_on, _cmach_desc, _cfx_process and _cfx_desc. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

/* The click action of the third param: nothing to do. */
static void go_click(void *arg, uint32_t track)
{
    (void)arg;
    (void)track;
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_CMACH, "Commit", "CMT");
    dk_page *p = dk_machine_add_page(m, "Commit", "CMT");
    dk_page_add_param(p, "Tone", "TONE", 64);
    dk_param *mode = dk_page_add_param(p, "Mode", "MODE", 0);
    dk_param_range(mode, 0, 3);
    dk_param_latched(mode);
    dk_param *go = dk_page_add_param(p, "Go", "GO", 0);
    dk_param_on_click(go, go_click, 0);

    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_CFX, "Commit FX", "CFX");
    dk_fx_routes(fx, DK_ROUTE_SEND | DK_ROUTE_MASTER, DK_ROUTE_SEND);
    dk_page *fp = dk_fx_add_page(fx, "Commit FX", "CFX");
    dk_page_add_fx_param(fp, "Gain", "GAIN", DK_PERCENT, 0.0f, 1.27f, 0.5f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
