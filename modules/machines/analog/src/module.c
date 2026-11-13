/* The machine of fadedlimes/analog, declared by builder calls in init: one machine `analog` with four parameters. The DSP reads the words in this
   order: DETUNE, WAVE, SUB, FOLD (src/dsp/analog.s); its symbols are _va_render, _va_note_on and _va_desc (`dsp = "va"` in module.toml).
   TUNE (0xca) and LEV (0xd1) are the stock rows. The four parameters take the device list's first four positions: 1, 2, 4, 5. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_ANALOG, "Analog", "ANA");
    dk_page *p = dk_machine_add_page(m, "Analog", "ANALOG");
    dk_page_add_param(p, "Detune", "DET", 40);
    dk_page_add_param(p, "Wave", "WAVE", 0);
    dk_page_add_param(p, "Sub", "SUB", 0);
    dk_page_add_param(p, "Fold", "FOLD", 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
