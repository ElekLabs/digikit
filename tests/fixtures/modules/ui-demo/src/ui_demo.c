/* test/ui-demo: every kind of row and view of <dk/platform/ui.h>, to see core/ui on the screen. A content module: in init it adds the SETTINGS row UI DEMO
 * (dk_ui_settings_row) that opens the demo menu, and registers the report command.
 *
 *   0x0f  no body: report; reply, big-endian words: 0 dk_demo_value, 4/8/12 dk_demo_checks[0..2], 16 dk_demo_events_n, 20 dk_demo_answer_v
 *         (the number is a fixture command of sdk.toml [protocol.ranges] `ui_views_test`: the two fixtures are never loaded together)
 */
#include <stdint.h>
#include <dk/core/bytes.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/ui.h>

static int32_t g_toggle = 1;
int32_t dk_demo_value = 64;
int dk_demo_checks[3];
int dk_demo_events_n;
int dk_demo_answer_v = -1;
static dk_ui *g_ui; /* core/ui's handle (dk_ui_open), set in init */
static dk_ui_menu *g_main, *g_page, *g_list, *g_empty;

static void on_answer(void *ctx, int32_t v)
{
    (void)ctx;
    dk_demo_answer_v = v;
    dk_demo_events_n++;
}

static void do_toast(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    dk_demo_events_n++;
    dk_ui_toast(g_ui, "HELLO FROM CORE UI");
}

static void do_confirm(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    dk_ui_confirm(g_ui, "DELETE EVERYTHING?", "REALLY? Y/N", on_answer, 0);
}

static void do_choice(void *ctx, int32_t arg)
{
    static const char *const items[] = {"SEND", "MASTER", "OFF"};
    (void)ctx;
    (void)arg;
    dk_ui_choice(g_ui, "ROUTE", items, 3, on_answer, 0);
}

static void changed(void *ctx, int32_t v)
{
    (void)ctx;
    (void)v;
    dk_demo_events_n++;
}

static void fill_page(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    dk_ui_menu_add_text(m, "A TEXT PAGE HAS NO");
    dk_ui_menu_add_text(m, "HIGHLIGHTED ROW.");
    dk_ui_menu_add_text(m, "THE KEYS SCROLL IT.");
    dk_ui_menu_add_text(m, "LINE FOUR");
    dk_ui_menu_add_text(m, "LINE FIVE");
    dk_ui_menu_add_text(m, "LINE SIX");
    dk_ui_menu_add_text(m, "LINE SEVEN");
    dk_ui_menu_add_text(m, "LINE EIGHT");
}

static void fill_list(dk_ui_menu *m, void *ctx)
{
    static const char *const names[] = {"FIRST ITEM", "SECOND ITEM", "THIRD ITEM"};
    (void)ctx;
    for (int i = 0; i < 3; i++)
        dk_ui_menu_add_text(m, names[i]);
}

static void fill_none(dk_ui_menu *m, void *ctx)
{
    (void)m;
    (void)ctx;
}

static void panel_act(void *ctx, int32_t arg)
{
    (void)ctx;
    dk_demo_events_n++;
    dk_ui_toast(g_ui, arg ? "PANEL: SECOND" : "PANEL: FIRST");
}

static void list_panel(dk_ui_menu *p, int32_t row, void *ctx)
{
    (void)row;
    (void)ctx;
    dk_ui_menu_add_action(p, "LOAD", panel_act, 0, 0);
    dk_ui_menu_add_action(p, "INFO", panel_act, 0, 1);
}

static void list_check(void *ctx, int32_t row, int32_t on)
{
    (void)ctx;
    if (row >= 0 && row < 3)
        dk_demo_checks[row] = on;
    dk_demo_events_n++;
}

/* NO is refused while the third item is checked (the list stays open). */
static int32_t list_back(void *ctx)
{
    (void)ctx;
    dk_demo_events_n++;
    return dk_demo_checks[2];
}

static const dk_ui_menu_desc page_desc = {
    .name = "UiDemoPage", .title = "TEXT PAGE", .flags = DK_UI_TEXT_PAGE, .fill = fill_page};
static const dk_ui_menu_desc list_desc = {.name = "UiDemoList",
                                          .title = "DEMO LIST",
                                          .flags = DK_UI_LIST,
                                          .fill = fill_list,
                                          .empty = "NO ROWS",
                                          .panel = list_panel,
                                          .check = list_check,
                                          .back = list_back};
static const dk_ui_menu_desc empty_desc = {.name = "UiDemoEmpty",
                                           .title = "EMPTY LIST",
                                           .flags = DK_UI_LIST,
                                           .fill = fill_none,
                                           .empty = "NOTHING HERE",
                                           .panel = list_panel,
                                           .check = list_check};

static void fill_main(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    dk_ui_menu_add_toggle(m, "TOGGLE", &g_toggle, changed, 0);
    dk_ui_menu_add_value(m, "VALUE", &dk_demo_value, 0, 127, changed, 0);
    dk_ui_menu_add_action(m, "TOAST", do_toast, 0, 0);
    dk_ui_menu_add_action(m, "CONFIRM", do_confirm, 0, 0);
    dk_ui_menu_add_action(m, "CHOICE", do_choice, 0, 0);
    dk_ui_menu_add_submenu(m, "TEXT PAGE", g_page);
    dk_ui_menu_add_submenu(m, "LIST", g_list);
    dk_ui_menu_add_submenu(m, "EMPTY LIST", g_empty);
    dk_ui_menu_add_text(m, "A TEXT ROW");
}

static const dk_ui_menu_desc main_desc = {
    .name = "UiDemoMenu", .title = "UI DEMO", .fill = fill_main};

static void open_main(void *arg)
{
    (void)arg;
    if (!g_main) {
        g_page = dk_ui_menu_new(g_ui, &page_desc);
        g_list = dk_ui_menu_new(g_ui, &list_desc);
        g_empty = dk_ui_menu_new(g_ui, &empty_desc);
        g_main = dk_ui_menu_new(g_ui, &main_desc);
    }
    if (g_main)
        dk_ui_menu_open(g_main);
}

static int32_t command(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    uint8_t *out = reply;
    (void)seq;
    (void)request;
    if (cmd != 0x0f || len != 0)
        return -50;
    dk_bytes_put32(out + 0, (uint32_t)dk_demo_value);
    for (uint32_t i = 0; i < 3; i++)
        dk_bytes_put32(out + 4 + 4 * i, (uint32_t)dk_demo_checks[i]);
    dk_bytes_put32(out + 16, (uint32_t)dk_demo_events_n);
    dk_bytes_put32(out + 20, (uint32_t)dk_demo_answer_v);
    return 24;
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_ui_open(ctx, &g_ui);
    if (r != DK_OK)
        return r;
    r = dk_ui_settings_row(g_ui, "UI DEMO", open_main, 0);
    if (r != DK_OK)
        return r;
    dk_host_register(0x0f, 1, command);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
