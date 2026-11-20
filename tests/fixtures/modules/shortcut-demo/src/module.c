/* test/shortcut-demo: one shortcut, hold [TRK] and press [YES] (dk_shortcut_add). The `shortcuts` service owns the kind. `on` and `off` count in the statics below, so a
   test (or a debugger) reads them. */
#include <dk/core/module.h>
#include <dk/platform/shortcuts.h>

static volatile uint32_t on_calls;
static volatile uint32_t off_calls;

static void on(void *arg)
{
    (void)arg;
    on_calls++;
}

static void off(void *arg)
{
    (void)arg;
    off_calls++;
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
