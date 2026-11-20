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

static void copy_text(char *out, uint32_t cap, const char *in)
{
    uint32_t i = 0;
    while (in && in[i] && i + 1 < cap) {
        out[i] = in[i];
        i++;
    }
    out[i] = 0;
}

/* The firmware screen is pushed from the state kept in the owner object. */
static int32_t push_now(ui_text_input *t)
{
    dk_ui_text_input_desc d = {t->title, t->initial, t->max_len, t->done, t->arg};
    int32_t r = t->fw->push(t->fw->fw, &d, t->initial, t->open);
    if (r != DK_OK) {
        t->open = 0;
        t->done = 0;
        t->pending = 0;
    } else {
        t->fw->emit(t->fw->fw, DK_LOG_TEXT_INPUT_OPENED, DK_LOG_F_KEEP, t->owner, t->max_len);
    }
    return r;
}

static void push_deferred(void *ctx, uint32_t serial)
{
    ui_text_input *t = ctx;
    if (t->open == serial && t->pending) {
        t->pending = 0;
        push_now(t);
    }
}

int32_t ui_text_open(ui_text_input *t, uintptr_t owner, const dk_ui_text_input_desc *d,
                     uint32_t screen_max)
{
    int32_t r = ui_text_input_check(d, screen_max);
    if (r != DK_OK)
        return r;
    if (t->open)
        return DK_E_BUSY;
    do
        t->serial++;
    while (t->serial == 0);
    /* the state first: the firmware may call back as soon as the screen is on the stack */
    copy_text(t->title, sizeof t->title, d->title);
    ui_text_input_initial(t->initial, d);
    t->max_len = d->max_len;
    t->done = d->done;
    t->arg = d->arg;
    t->owner = owner;
    t->open = t->serial;
    t->pending = 0;
    if (ui_defer_active(t->defer)) {
        /* a key callback on a press: the screen would take the release of the key as its own YES; it comes after the release */
        r = ui_defer_add(t->defer, owner, push_deferred, t, t->open);
        if (r != DK_OK) {
            t->open = 0;
            t->done = 0;
            return r;
        }
        t->pending = 1;
        return DK_OK;
    }
    return push_now(t);
}

void ui_text_done(ui_text_input *t, const char *text)
{
    void (*done)(void *, const char *) = t->done;
    void *arg = t->arg;
    t->open = 0;
    t->done = 0;
    t->fw->emit(t->fw->fw, DK_LOG_TEXT_INPUT_DONE, DK_LOG_F_KEEP, t->owner, 0);
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
    if (!t->open || t->owner != owner)
        return;
    if (t->pending) {
        /* not on the screen yet: nothing to leave, the input is free */
        ui_defer_cancel(t->defer, push_deferred, t, t->open);
        t->pending = 0;
        t->open = 0;
    }
    t->done = 0;
}
