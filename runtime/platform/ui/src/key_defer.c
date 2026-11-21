/* Actions that wait for a key release (key_defer.h). Pure logic. */
#include "key_defer.h"
#include <dk/core/status.h>

#ifndef DK_OK
#define DK_OK 0
#endif

void ui_defer_enter(ui_defer *d, uint32_t code)
{
    d->in_key = 1;
    d->code = code;
}

void ui_defer_leave(ui_defer *d)
{
    d->in_key = 0;
}

int ui_defer_active(const ui_defer *d)
{
    return d->in_key;
}

int32_t ui_defer_add(ui_defer *d, uintptr_t owner, void (*run)(void *ctx, uint32_t arg), void *ctx,
                     uint32_t arg)
{
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++) {
        ui_defer_item *it = &d->item[i];
        if (it->used)
            continue;
        if (d->start && d->start(d->start_ctx) != 0)
            return DK_E_NOMEM;
        it->run = run;
        it->ctx = ctx;
        it->arg = arg;
        it->owner = owner;
        it->code = d->code;
        it->released = 0;
        it->idle = 0;
        it->used = 1;
        return DK_OK;
    }
    return DK_E_NOMEM;
}

int ui_defer_released(ui_defer *d, uint32_t code)
{
    int n = 0;
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++)
        if (d->item[i].used && d->item[i].code == code) {
            d->item[i].released = 1;
            n = 1;
        }
    return n;
}

void ui_defer_poll(ui_defer *d, int (*down)(void *kctx, uint32_t code), void *kctx)
{
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++) {
        ui_defer_item *it = &d->item[i];
        void (*run)(void *, uint32_t);
        void *ctx;
        uint32_t arg;
        if (!it->used)
            continue;
        if (!it->released) {
            if (down(kctx, it->code)) {
                it->idle = 0;
                continue;
            }
            if (++it->idle < UI_DEFER_IDLE_POLLS)
                continue;
        }
        run = it->run;
        ctx = it->ctx;
        arg = it->arg;
        it->used = 0; /* first: the action may queue another */
        run(ctx, arg);
    }
}

int ui_defer_cancel(ui_defer *d, void (*run)(void *ctx, uint32_t arg), void *ctx, uint32_t arg)
{
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++) {
        ui_defer_item *it = &d->item[i];
        if (it->used && it->run == run && it->ctx == ctx && it->arg == arg) {
            it->used = 0;
            return 1;
        }
    }
    return 0;
}

uint32_t ui_defer_drop_owner(ui_defer *d, uintptr_t owner)
{
    uint32_t n = 0;
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++)
        if (d->item[i].used && d->item[i].owner == owner) {
            d->item[i].used = 0;
            n++;
        }
    return n;
}

uint32_t ui_defer_pending(const ui_defer *d)
{
    uint32_t n = 0;
    for (uint32_t i = 0; i < UI_DEFER_MAX; i++)
        n += d->item[i].used;
    return n;
}
