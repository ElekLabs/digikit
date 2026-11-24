/* The open dialog's slot (dialog_slot.h). Pure logic: no firmware, no runtime. */
#include "dialog_slot.h"

int ui_dialog_is_open(const ui_dialog_slot *s)
{
    return s->serial != 0;
}

int32_t ui_dialog_begin(ui_dialog_slot *s, uint32_t serial, uintptr_t owner,
                        ui_dialog_done_fn *done, void *arg)
{
    if (s->serial || !serial)
        return -1;
    s->serial = serial;
    s->owner = owner;
    s->done = done;
    s->arg = arg;
    return 0;
}

static void clear(ui_dialog_slot *s)
{
    s->serial = 0;
    s->owner = 0;
    s->done = 0;
    s->arg = 0;
}

void ui_dialog_detach(ui_dialog_slot *s, uint32_t serial)
{
    if (s->serial == serial)
        clear(s);
}

int ui_dialog_answer(ui_dialog_slot *s, int32_t answer)
{
    ui_dialog_done_fn *done = s->done;
    void *arg = s->arg;
    clear(s);
    if (!done)
        return 0;
    done(arg, answer);
    return 1;
}

int ui_dialog_drop_owner(ui_dialog_slot *s, uintptr_t owner)
{
    if (!s->serial || !s->done || s->owner != owner)
        return 0;
    s->done = 0;
    s->arg = 0;
    return 1;
}
