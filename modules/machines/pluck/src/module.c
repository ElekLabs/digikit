/* The machine of digikit/pluck, declared by builder calls in init: one machine `pluck` with four parameters. The DSP reads the words in
   this order: DECAY, PICK, BRIGHT, COLOR (src/dsp/pluck.s); its symbols are _pk_render, _pk_note_on and _pk_desc (`dsp = "pk"` in module.toml). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_PLUCK, "Pluck", "PLK");
    dk_page *p = dk_machine_add_page(m, "Pluck", "PLUCK");
    dk_page_add_param(p, "Decay", "DECAY", 64);
    dk_page_add_param(p, "Pick position", "PICK", 32);
    dk_page_add_param(p, "Brightness", "BRIGHT", 80);
    dk_page_add_param(p, "Excitation colour", "COLOR", 40);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
