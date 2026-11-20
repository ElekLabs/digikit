/* test/shortcut-demo: one shortcut, hold [TRK] and press [YES] (dk_shortcut_add). The `shortcuts` service owns the kind. `on` and `off` log one error-level event each
   (an error is kept even when debug is off): "on" with the selected track (dk_selected_track, 0 to 15) as the value, "off" with 0, so a device test observes the calls and
   the track that was selected when the combination completed. */
#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/platform/shortcuts.h>

static void on(void *arg)
{
    (void)arg;
    dk_log(0, "on", (uint32_t)dk_selected_track());
}

static void off(void *arg)
{
    (void)arg;
    dk_log(0, "off", 0);
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_shortcut_def def = {
        .id = "hint",
        .label = "TRACK NAME HINT",
        .keys = {DK_KEY_TRK, DK_KEY_YES},
        .on = on,
        .off = off,
        .arg = 0,
    };
    return dk_shortcut_add(ctx, &def);
}

const dk_module_def dk_module = {.init = init};
