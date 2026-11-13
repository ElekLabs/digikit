/* The machine of test/dyn-dsp-hang, declared by a builder call in init: one machine `hang` without parameters.
   The DSP symbols are _hg_render, _hg_note_on and _hg_desc (`dsp = "hg"` in module.toml). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_HANG, "Hang", "HNG");
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
