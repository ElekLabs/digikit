/* The SETTINGS rows registry (settings_rows.h). Pure logic: no firmware, no runtime. */
#include "settings_rows.h"
#include <dk/core/status.h>

#ifndef DK_OK
#define DK_OK 0 /* as <dk/core/module.h> */
#endif

/* Field by field: a struct assignment would call memcpy, which this module does not link. */
static void move_row(ui_settings_row *d, const ui_settings_row *s)
{
    d->open = s->open;
    d->arg = s->arg;
    d->owner = s->owner;
    d->key = s->key;
    for (uint32_t i = 0; i < DK_UI_ROW; i++)
        d->text[i] = s->text[i];
}

static int same_name(const char *a, const char *b)
{
    for (uint32_t i = 0; i < DK_UI_MODULE_NAME; i++) {
        if (a[i] != b[i])
            return 0;
        if (!a[i])
            return 1;
    }
    return 1;
}

/* The key of (name, index): the one that exists, or a free one. 0 when all are used. */
static const ui_row_key *key_of(ui_settings_rows *s, const char *name, uint32_t index)
{
    ui_row_key *free_key = 0;
    for (uint32_t i = 0; i < DK_UI_SETTINGS_ROWS * 2; i++) {
        ui_row_key *k = &s->key[i];
        if (!k->used) {
            if (!free_key)
                free_key = k;
        } else if (k->index == index && same_name(k->name, name)) {
            return k;
        }
    }
    if (free_key) {
        uint32_t i = 0;
        while (name[i] && i + 1 < DK_UI_MODULE_NAME) {
            free_key->name[i] = name[i];
            i++;
        }
        free_key->name[i] = 0;
        free_key->index = index;
        free_key->used = 1;
    }
    return free_key;
}

int ui_owner_name(uintptr_t handle, ui_record_name_fn *record_name, char out[DK_UI_MODULE_NAME])
{
    uint8_t name[DK_UI_MODULE_NAME];
    out[0] = 0;
    if (handle == 0 || record_name((uint32_t)(handle - 1), name) != 0)
        return 0;
    if (name[DK_UI_MODULE_NAME - 1] != 0)
        return 0;
    for (uint32_t k = 0; k < DK_UI_MODULE_NAME; k++)
        out[k] = (char)name[k];
    return 1;
}

int32_t ui_rows_add(ui_settings_rows *s, uintptr_t owner, const char *name, int in_init,
                    const char *text, dk_ui_settings_open_fn *open, void *arg)
{
    ui_settings_row *r;
    const ui_row_key *key;
    uint32_t i = 0, index = 0;
    if (!in_init)
        return DK_E_CONTEXT;
    if (!text || !open || !name || !name[0])
        return DK_E_CONTRACT;
    if (s->n == DK_UI_SETTINGS_ROWS)
        return DK_E_NOMEM;
    for (i = 0; i < s->n; i++)
        if (s->row[i].owner == owner)
            index++;
    key = key_of(s, name, index);
    if (!key)
        return DK_E_NOMEM;
    r = &s->row[s->n];
    i = 0;
    while (text[i] && i + 1 < DK_UI_ROW) {
        r->text[i] = text[i];
        i++;
    }
    r->text[i] = 0;
    r->open = open;
    r->arg = arg;
    r->owner = owner;
    r->key = key;
    s->n++;
    if (s->menu)
        s->dirty = 1;
    return DK_OK;
}

uint32_t ui_rows_remove_owner(ui_settings_rows *s, uintptr_t owner)
{
    uint32_t from, to = 0;
    for (from = 0; from < s->n; from++) {
        if (s->row[from].owner == owner)
            continue;
        if (to != from)
            move_row(&s->row[to], &s->row[from]);
        to++;
    }
    from = s->n - to;
    for (uint32_t i = to; i < s->n; i++)
        s->row[i].open = 0;
    s->n = to;
    if (from && s->menu)
        s->dirty = 1;
    return from;
}

int ui_rows_run(const ui_settings_rows *s, const ui_row_key *key)
{
    for (uint32_t i = 0; i < s->n; i++)
        if (s->row[i].key == key) {
            s->row[i].open(s->row[i].arg);
            return 1;
        }
    return 0;
}

void ui_rows_built(ui_settings_rows *s, uint8_t *menu)
{
    s->menu = menu;
    s->dirty = 0;
}

int ui_rows_dirty(const ui_settings_rows *s)
{
    return s->menu && s->dirty;
}

void ui_rows_gone(ui_settings_rows *s, const uint8_t *menu)
{
    if (s->menu == menu) {
        s->menu = 0;
        s->dirty = 0;
    }
}

int ui_rows_pass(ui_settings_rows *s, ui_rows_rebuild_fn *rebuild, uint32_t vtable)
{
    /* read once: the destructor hook may clear it from another task */
    uint8_t *volatile menu = s->menu;
    if (!menu || !s->dirty)
        return 0;
    /* clear first: a rebuild that fails to clear it must not run every pass */
    s->dirty = 0;
    if (!rebuild || !vtable) {
        /* no rebuild or no vtable fact on this firmware: do not call, forget the pointer */
        int first = !s->absent_seen;
        s->absent_seen = 1;
        s->menu = 0;
        return first ? -1 : 0;
    }
    /* the kept pointer is no SETTINGS list: do not call, forget it */
    if (*(const uint32_t *)menu != vtable) {
        s->menu = 0;
        return 0;
    }
    rebuild(menu);
    return 1;
}
