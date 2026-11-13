/* The effect of __ID__, declared by builder calls in init: one effect with one GAIN knob and the framework's MIX knob. Saved kits keep the effect by
   module id + effect id: never rename either. */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"
/* Trace marks for C code: see sdk/README.md, "Tracing" (`<dk/core/trace.h>`, `DK_TRACE_LOG`). The DSP side uses `DK_STEP` of dk_trace.inc. */

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID___UPPER__, "__TITLE__", "__SHORT__");
    dk_page *p = dk_fx_add_page(fx, "__TITLE__", "__TAB__");
    dk_page_add_fx_param(p, "Gain", "GAIN", DK_PERCENT, 0.0f, 1.27f, 0.5f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
