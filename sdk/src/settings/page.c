/* The settings page: see <dk/settings/page.h>. The statics hold the one page of the module. */
#include <dk/settings/page.h>
#include "field.h"

static dk_settings_page_desc g_copy;          /* the descriptor, copied: the module's may be on its stack */
static const dk_settings_page_desc *g_desc;   /* &g_copy once init succeeded */
static dk_settings *g_settings;
static dk_ui *g_ui;
static dk_ui_menu *g_menu;       /* the page being shown (set by fill) */
static int32_t g_value[DK_SETTINGS_FIELDS];
static int32_t g_saved[DK_SETTINGS_FIELDS];
static uint8_t g_failed;         /* the last save failed */
static uint8_t g_restart;        /* a restart field changed */

static void refresh(void)
{
    if (g_menu)
        dk_ui_menu_refresh(g_menu);
}

static int32_t save(uint32_t i, int32_t value)
{
    char text[8];
    uint32_t n = dk_settings_field_encode(value, text);
    return dk_settings_put(g_settings, g_desc->fields[i].key, text, n);
}

/* The value of field i is `value` (a toggle, a value row or the popup wrote or chose it): save it and tell the module, or put the saved value back. */
static void commit(uint32_t i, int32_t value)
{
    if (!g_desc || i >= g_desc->count)
        return;
    value = dk_settings_field_clamp(&g_desc->fields[i], value);
    g_value[i] = value;
    if (save(i, value) != DK_OK) {
        g_value[i] = g_saved[i];
        g_failed = 1;
        refresh();
        return;
    }
    g_saved[i] = value;
    g_failed = 0;
    if (g_desc->fields[i].flags & DK_SETTINGS_RESTART)
        g_restart = 1;
    refresh();
    if (g_desc->changed)
        g_desc->changed(g_desc->arg, i, value);
}

static void on_row(void *arg, int32_t value)
{
    commit((uint32_t)(uintptr_t)arg, value);
}

static void on_pick(void *arg, int32_t index)
{
    commit((uint32_t)(uintptr_t)arg, index);
}

static void open_choice(void *arg, int32_t field)
{
    const dk_settings_field *f;
    (void)arg;
    if (!g_desc || !g_ui || field < 0 || (uint32_t)field >= g_desc->count)
        return;
    f = &g_desc->fields[field];
    dk_ui_choice(g_ui, f->label, f->labels, f->max + 1, on_pick, (void *)(uintptr_t)field);
}

int32_t dk_settings_page_init(const dk_settings_page_desc *desc, dk_settings *settings, dk_ui *ui)
{
    int32_t rc;
    g_desc = 0;
    g_menu = 0;
    g_failed = g_restart = 0;
    if (!desc || !settings)
        return DK_E_MISSING;
    rc = dk_settings_fields_check(desc->fields, desc->count);
    if (rc != DK_OK)
        return rc;
    for (uint32_t i = 0; i < desc->count; i++)
        if (desc->fields[i].kind == DK_SETTINGS_ENUM && !ui)
            return DK_E_MISSING;
    for (uint32_t i = 0; i < desc->count; i++) {
        char text[8];
        int32_t n = dk_settings_get(settings, desc->fields[i].key, text, sizeof text);
        g_value[i] = n > 0 ? dk_settings_field_decode(&desc->fields[i], text, (uint32_t)n) : desc->fields[i].def;
        g_saved[i] = g_value[i];
    }
    g_copy = *desc;
    g_desc = &g_copy;
    g_settings = settings;
    g_ui = ui;
    return DK_OK;
}

void dk_settings_page_fill(dk_ui_menu *menu)
{
    if (!g_desc) {
        dk_ui_menu_add_text(menu, "NO SETTINGS");
        return;
    }
    g_menu = menu;
    for (uint32_t i = 0; i < g_desc->count; i++) {
        const dk_settings_field *f = &g_desc->fields[i];
        void *arg = (void *)(uintptr_t)i;
        if (f->kind == DK_SETTINGS_BOOL) {
            dk_ui_menu_add_toggle(menu, f->label, &g_value[i], on_row, arg);
        } else if (f->kind == DK_SETTINGS_INT) {
            dk_ui_menu_add_value(menu, f->label, &g_value[i], f->min, f->max, on_row, arg);
        } else {
            char text[DK_UI_ROW];
            dk_settings_field_row(f, g_value[i], text);
            dk_ui_menu_add_action(menu, text, open_choice, 0, (int32_t)i);
        }
    }
    if (g_failed)
        dk_ui_menu_add_text(menu, "NOT SAVED");
    if (g_restart)
        dk_ui_menu_add_text(menu, "ON NEXT RESTART");
}

int32_t dk_settings_page_value(uint32_t field)
{
    return g_desc && field < g_desc->count ? g_value[field] : 0;
}

int32_t dk_settings_page_set(uint32_t field, int32_t value)
{
    int32_t rc;
    if (!g_desc || field >= g_desc->count)
        return DK_E_NOT_FOUND;
    value = dk_settings_field_clamp(&g_desc->fields[field], value);
    rc = save(field, value);
    if (rc != DK_OK)
        return rc;
    g_value[field] = g_saved[field] = value;
    refresh();
    return DK_OK;
}
