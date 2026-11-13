/* The machine of test/sine, declared by builder calls in init: one machine `sine` with one level parameter and a page of one knob.
   The DSP symbols are _sn_render, _sn_note_on and _sn_desc (`dsp = "sn"` in module.toml). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_SINE, "Sine", "SIN");
    dk_page *p = dk_machine_add_page(m, "Sine", "SINE");
    dk_page_add_param(p, "Level", "LEVEL", 127);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
