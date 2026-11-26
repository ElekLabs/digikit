/* test/settings-page: the settings page of <dk/settings/page.h> from one field table. The module builds no menu: `dk_module.menu` fills the page that
 * DIGIKIT > MODULES > Settings page > SETTINGS... opens, and the stored values are read in `init`.
 *
 *   0x0f  no body: report; reply, big-endian words: 0 loud, 4 gain, 8 mode, 12 the number of `changed` calls, 16 the field of the last call, 20 its value
 *         (the number is a fixture command of sdk.toml [protocol.ranges] `ui_views_test`: the two fixtures are never loaded together)
 */
#include <stdint.h>
#include <dk/core/bytes.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/settings.h>
#include <dk/platform/ui.h>
#include <dk/settings/page.h>

enum { LOUD, GAIN, MODE };

static const char *const MODES[] = {"OFF", "SLOW", "FAST"};
static const dk_settings_field FIELDS[] = {
    {.key = "loud", .label = "LOUD", .kind = DK_SETTINGS_BOOL, .def = 1},
    {.key = "gain", .label = "GAIN", .kind = DK_SETTINGS_INT, .def = 50, .min = 0, .max = 100},
    {.key = "mode", .label = "MODE", .kind = DK_SETTINGS_ENUM, .def = 1, .max = 2, .labels = MODES},
};

static int32_t g_calls, g_last_field, g_last_value;

static void changed(void *arg, uint32_t field, int32_t value)
{
    (void)arg;
    g_calls++;
    g_last_field = (int32_t)field;
    g_last_value = value;
}

static int32_t command(uint32_t cmd, uint32_t seq, const void *body, uint32_t len, void *out)
{
    uint8_t *o = out;
    (void)cmd;
    (void)seq;
    (void)body;
    (void)len;
    dk_bytes_put32(o, (uint32_t)dk_settings_page_value(LOUD));
    dk_bytes_put32(o + 4, (uint32_t)dk_settings_page_value(GAIN));
    dk_bytes_put32(o + 8, (uint32_t)dk_settings_page_value(MODE));
    dk_bytes_put32(o + 12, (uint32_t)g_calls);
    dk_bytes_put32(o + 16, (uint32_t)g_last_field);
    dk_bytes_put32(o + 20, (uint32_t)g_last_value);
    return 24;
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_settings *settings;
    dk_ui *ui;
    dk_settings_page_desc d = {.fields = FIELDS, .count = 3, .changed = changed};
    int32_t r = dk_settings_open(ctx, &settings);
    if (r != DK_OK)
        return r;
    r = dk_ui_open(ctx, &ui);
    if (r != DK_OK)
        return r;
    r = dk_settings_page_init(&d, settings, ui);
    if (r != DK_OK)
        return r;
    return dk_host_register(0x0f, 1, command);
}

static void menu(dk_module_ctx *ctx, dk_ui_menu *m)
{
    (void)ctx;
    dk_settings_page_fill(m);
}

const dk_module_def dk_module = {.init = init, .menu = menu};
