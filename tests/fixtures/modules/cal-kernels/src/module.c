/* The machines of test/cal-kernels, declared by builder calls in init: five machines with no parameter and no page, one kernel each.
   The DSP symbols are _ka_*, _kd_*, _km_*, _kb_* and _kx_* (`dsp` in module.toml). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_ALU, "Cal ALU", "KAL");
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_DM, "Cal DM", "KDM");
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_MEM, "Cal MEM", "KMM");
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_BRANCH, "Cal BRANCH", "KBR");
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_MIX, "Cal MIX", "KMX");
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
