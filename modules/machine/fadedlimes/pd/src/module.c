/* The machine of fadedlimes/pd, declared by builder calls in init: one machine `pd` with four parameters. The DSP reads the words in this
   order: AMOUNT, CURVE, DETUNE, FOLD (src/dsp/pd.s).
   SAMPLE's TUNE (0xca) and LEV (0xd1) are at the ends. The DSP reads params[i] from the knob at param_slots[i]
   (Digitakt II 1.17 and Digitone II 1.12: 1, 2, 4, 5). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_PD, "Phase Dist", "PD");
    dk_page *p = dk_machine_add_page(m, "PHASE DIST", "PD");
    dk_page_add_param(p, "Distortion", "PD", 0);
    dk_page_add_param(p, "Curve", "CURVE", 0);
    dk_page_add_param(p, "Detune", "DET", 40);
    dk_page_add_param(p, "Fold", "FOLD", 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
