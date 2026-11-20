/* digikit/track-manager: names for the tracks. `init` loads the names, adds the SETTINGS row TRACK MANAGER and the shortcut [TRK]+[YES] that shows the name
 * of the active track while it is held. The names are saved in the module's settings under the key "names". */

#include <dk/core/module.h>
#include <dk/platform/settings.h>
#include <dk/platform/shortcuts.h>
#include <dk/platform/ui.h>

#include "hint_overlay.h"
#include "manager_screen.h"
#include "track_names.h"

#define SETTINGS_ROW_TEXT "TRACK MANAGER"

static track_names names;
static dk_settings *settings;

static int32_t init(dk_module_ctx *ctx)
{
    track_names_init(&names, dk_device_track_count());
    if (dk_settings_open(ctx, &settings) == DK_OK) {
        uint8_t value[TRACK_NAMES_ENCODED_MAX];
        int32_t len = dk_settings_get(settings, TRACK_NAMES_KEY, value, sizeof value);
        if (len >= 0)
            track_names_load(&names, value, (uint32_t)len);
    } else {
        settings = 0;
    }
    dk_ui *ui = 0;
    int32_t r = dk_ui_open(ctx, &ui);
    if (r != DK_OK)
        return r;
    manager_screen_init(ui, &names, settings);
    hint_overlay_init(ui, &names);
    r = dk_ui_settings_row(ui, SETTINGS_ROW_TEXT, manager_screen_open, 0);
    if (r != DK_OK)
        return r;
    static const dk_shortcut_def hint = {
        .id = "hint",
        .label = "TRACK NAME HINT",
        .keys = {DK_KEY_TRK, DK_KEY_YES},
        .on = hint_overlay_show,
        .off = hint_overlay_hide,
    };
    return dk_shortcut_add(ctx, &hint);
}

/* The runtime closes the module's views when it unloads. */
static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
}

const dk_module_def dk_module = {.init = init, .fini = fini};
