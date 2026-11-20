/* The machine of test/dyn-dn2, declared by builder calls in init: machine `dyn_pluck` with one page of four knobs. The DSP is
   test/sine's (`dsp = "sn"` in module.toml): it reads parameter word 0 only, the other three words are not read. */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_DYN_PLUCK, "DynSyn", "DSY");
    dk_page *p = dk_machine_add_page(m, "Dyn Synth", "DYN");
    dk_param *a = dk_page_add_param(p, "Dyn Alpha", "ALPH", 32);
    (void)a;
    dk_param *b = dk_page_add_param(p, "Dyn Beta", "BETA", 16);
    dk_param_range(b, 0, 63);
    dk_param *c = dk_page_add_param(p, "Dyn Gamma", "GAMA", 112);
    dk_param_range(c, 1, 127);
    dk_page_add_param(p, "Dyn Delta", "DELT", 64);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
