/* The effect of freqshift, declared by builder calls in init: one effect `fshift` with one knob. Its DSP symbols are _fs_process and _fs_desc
   (`dsp = "fs"` in module.toml).
   Saved kits keep the effect by module id + effect id: never rename either.
   The page comes after CHO in the FX view, with the framework's MIX knob. The effect starts as a send effect:
   a send has no dry path of its own, so MIX is the return level and starts at 127 (0 would add nothing).
   The user may move it to the master; there MIX 127 is the fully shifted sound. MIX is one value for both routes.
   SHIFT is bipolar: the shown value -64 .. +63 is the float the routine reads. 0 is no shift; |v| = 1 is about 0.1 Hz,
   the ends are about +-2 kHz, exponential in between (the table is in docs/findings/54-module-cookbook.md). */
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_FSHIFT, "Freq Shifter", "FSH");
    dk_fx_routes(fx, DK_ROUTE_SEND | DK_ROUTE_MASTER, DK_ROUTE_SEND);
    dk_page *p = dk_fx_add_page(fx, "Freq Shifter", "FSH");
    dk_page_add_fx_param(p, "Freq Shift", "SHIFT", DK_BIPOLAR, -64.0f, 63.0f, 0.0f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
