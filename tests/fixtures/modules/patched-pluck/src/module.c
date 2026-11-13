/* The machine of test/patched-pluck, declared by builder calls in init: the same machine as digikit/pluck, patched in. The DSP reads
   the words in this order: DECAY, PICK, BRIGHT, COLOR. The defaults give a medium-length, slightly damped, lightly combed pluck. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_PLUCK, "Pluck", "PLK");
    dk_page *p = dk_machine_add_page(m, "Pluck");
    dk_page_add_param(p, "Decay", "DECAY", 64);
    dk_page_add_param(p, "Pick position", "PICK", 32);
    dk_page_add_param(p, "Brightness", "BRIGHT", 80);
    dk_page_add_param(p, "Excitation colour", "COLOR", 40);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
