/* The machine of test/dyn-guard-nan, declared by builder calls in init: one machine `guardnan` with one parameter and a page of one
   knob (a machine without a page has nothing to show). The render is the same body on both devices. The DSP symbols are _gn_render,
   _gn_note_on and _gn_desc (`dsp = "gn"` in module.toml). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_GUARDNAN, "NaN fixture", "NAN");
    dk_page *p = dk_machine_add_page(m, "NaN fixture", "NAN");
    dk_page_add_param(p, "Nan alpha", "ALPH", 32);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
