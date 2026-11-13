/* The machine of fadedlimes/bd, declared by builder calls in init: one machine `bd` with four parameters. The DSP reads the words in this
   order: BEND, INDEX, RATIO, NOISE (src/dsp/bd.s).
   TUNE (slot 0) is the kick's note. The parameters take the first positions of the device's param_slots list:
   1, 2 and 4 on the Digitakt II. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_BD, "Bass Drum", "BD");
    dk_page *p = dk_machine_add_page(m, "BASS DRUM", "BD");
    dk_page_add_param(p, "Bend", "BEND", 64);
    dk_page_add_param(p, "FM Index", "INDEX", 32);
    dk_page_add_param(p, "Ratio", "RATIO", 0);
    dk_page_add_param(p, "Noise", "NOISE", 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
