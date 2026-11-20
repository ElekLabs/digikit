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
    d->id = s->id;
    for (uint32_t i = 0; i < DK_UI_ROW; i++)
        d->text[i] = s->text[i];
}

int32_t ui_rows_add(ui_settings_rows *s, uintptr_t owner, const char *text,
                    dk_ui_settings_open_fn *open, void *arg)
{
    ui_settings_row *r;
    uint32_t i = 0;
    if (!text || !open)
        return DK_E_CONTRACT;
    if (s->n == DK_UI_SETTINGS_ROWS)
        return DK_E_NOMEM;
    r = &s->row[s->n];
    while (text[i] && i + 1 < DK_UI_ROW) {
        r->text[i] = text[i];
        i++;
    }
    r->text[i] = 0;
    r->open = open;
    r->arg = arg;
    r->owner = owner;
    do
        r->id = ++s->last_id;
    while (r->id == 0);
    s->n++;
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
    return from;
}

int ui_rows_run(const ui_settings_rows *s, uint32_t id)
{
    for (uint32_t i = 0; i < s->n; i++)
        if (s->row[i].id == id) {
            s->row[i].open(s->row[i].arg);
            return 1;
        }
    return 0;
}
