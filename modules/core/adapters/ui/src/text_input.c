/* The checks and copy of the dk_ui_text_input desc (text_input.h). Pure logic. */
#include "text_input.h"
#include <dk/core/status.h>

#ifndef DK_OK
#define DK_OK 0 /* as <dk/core/module.h> */
#endif

int32_t ui_text_input_check(const dk_ui_text_input_desc *d, uint32_t screen_max)
{
    if (!d || !d->done || !d->title)
        return DK_E_CONTRACT;
    if (d->max_len == 0 || d->max_len > DK_UI_TEXT_INPUT_MAX || d->max_len > screen_max)
        return DK_E_CONTRACT;
    return DK_OK;
}

void ui_text_input_initial(char out[DK_UI_TEXT_INPUT_MAX + 1], const dk_ui_text_input_desc *d)
{
    uint32_t i = 0;
    if (d->initial)
        while (d->initial[i] && i < d->max_len) {
            out[i] = d->initial[i];
            i++;
        }
    out[i] = 0;
}

int32_t ui_text_open(ui_text_input *t, uintptr_t owner, const dk_ui_text_input_desc *d,
                     uint32_t screen_max)
{
    char initial[DK_UI_TEXT_INPUT_MAX + 1];
    int32_t r = ui_text_input_check(d, screen_max);
    if (r != DK_OK)
        return r;
    if (t->open)
        return DK_E_BUSY;
    ui_text_input_initial(initial, d);
    do
        t->serial++;
    while (t->serial == 0);
    /* the state first: the firmware may call back as soon as the screen is on the stack */
    t->done = d->done;
    t->arg = d->arg;
    t->owner = owner;
    t->open = t->serial;
    r = t->fw->push(t->fw->fw, d, initial, t->serial);
    if (r != DK_OK) {
        t->open = 0;
        t->done = 0;
        return r;
    }
    return DK_OK;
}

void ui_text_done(ui_text_input *t, const char *text)
{
    void (*done)(void *, const char *) = t->done;
    void *arg = t->arg;
    t->open = 0;
    t->done = 0;
    if (done)
        done(arg, text);
}

void ui_text_gone(ui_text_input *t, uint32_t serial)
{
    if (t->open == serial) {
        t->open = 0;
        t->done = 0;
    }
}

void ui_text_drop_owner(ui_text_input *t, uintptr_t owner)
{
    if (t->open && t->owner == owner)
        t->done = 0;
}
