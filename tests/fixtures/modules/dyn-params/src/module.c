/* The machine of test/dyn-params, declared by builder calls in init: one machine `dynp`. The DSP is test/sine's (`dsp = "sn"`). */
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_DYNP, "DynP", "DYP");
    /* TODO(S3): four parameters (words 0 .. 3) on no page, three with fractional min/max/default, which dk_page_add_param and
       dk_param_range (uint32_t) cannot express. Today's declarations:
       word 0 "Param 0" "P0"  0 .. 127, default 16
       word 1 "Param 1" "P1"  1/256 .. 126, default 4369/256
       word 2 "Param 2" "P2"  2/256 .. 125, default 4642/256
       word 3 "Param 4" "P4"  4/256 .. 123, default 5188/256 */
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
