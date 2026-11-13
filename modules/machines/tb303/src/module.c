/* The machine of tom/tb303, declared by builder calls in init: one machine `tb303` with six parameters. The DSP reads the words in this
   order: WAVE, ACC, CUT, RES, ENV, DEC (src/dsp/tb303.s); its symbols start with `_tb_` (`dsp = "tb"` in module.toml). On the Digitakt II they take
   knobs 1, 2, 4, 5, 6 and 7; DEC on knob 7 (LEV) gives the track a fixed level. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_TB303, "TB303", "303");
    dk_page *p = dk_machine_add_page(m, "TB303", "303");
    dk_param *wave = dk_page_add_param(p, "Wave", "WAVE", 0);
    dk_param_range(wave, 0, 1);
    dk_page_add_param(p, "Accent", "ACC", 127);
    dk_page_add_param(p, "Cutoff", "CUT", 64);
    dk_page_add_param(p, "Resonance", "RES", 64);
    dk_page_add_param(p, "Env Mod", "ENV", 64);
    dk_page_add_param(p, "Decay", "DEC", 64);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
