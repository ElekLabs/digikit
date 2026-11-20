/* The pool of module screens (view_pool.h). Pure logic over the `ui_view_fw` interface. */
#include "view_pool.h"

#define HANDLE_BITS 8u

static uintptr_t handle_of(const ui_view *v, uint32_t slot)
{
    return ((uintptr_t)v->count << HANDLE_BITS) | (slot + 1);
}

/* The callbacks and their argument; the flags stay, as they say how the firmware view treats keys until it is gone. */
static void drop_callbacks(ui_view *v)
{
    v->desc.draw = 0;
    v->desc.key = 0;
    v->desc.closed = 0;
    v->desc.arg = 0;
}

static void clear_desc(ui_view *v)
{
    v->desc.flags = 0;
    drop_callbacks(v);
}

void ui_views_init(ui_views *p, const ui_view_fw *fw, ui_defer *defer)
{
    p->fw = fw;
    p->defer = defer;
    for (uint32_t i = 0; i < DK_UI_VIEWS; i++) {
        p->v[i].state = UI_VIEW_FREE;
        p->v[i].count = 0;
        p->v[i].owner = 0;
        p->v[i].fw_view = 0;
        clear_desc(&p->v[i]);
    }
}

static void push_deferred(void *ctx, uint32_t slot);
static void close_deferred(void *ctx, uint32_t slot);

/* The firmware view of slot `slot` is made and pushed. 0 when it could not be made: the slot is free again. */
static int push_slot(ui_views *p, uint32_t slot)
{
    ui_view *v = &p->v[slot];
    v->fw_view = p->fw->push(p->fw->fw, slot, (v->desc.flags & DK_UI_VIEW_OVERLAY) != 0);
    if (!v->fw_view) {
        v->state = UI_VIEW_FREE;
        clear_desc(v);
        return 0;
    }
    v->state = UI_VIEW_OPEN;
    p->fw->emit(p->fw->fw, DK_LOG_VIEW_OPENED, DK_LOG_F_KEEP, v->owner, v->desc.flags, slot);
    return 1;
}

uintptr_t ui_views_open(ui_views *p, uintptr_t owner, const dk_ui_view_desc *d)
{
    uint32_t slot;
    ui_view *v = 0;
    if (!d || !d->draw || (d->flags & ~(uint32_t)DK_UI_VIEW_OVERLAY))
        return 0;
    for (slot = 0; slot < DK_UI_VIEWS; slot++)
        if (p->v[slot].state == UI_VIEW_FREE) {
            v = &p->v[slot];
            break;
        }
    if (!v)
        return 0;
    if (p->fw->poll_start(p->fw->fw) != 0)
        return 0;
    /* the slot is taken before the push: the firmware may call back into it as soon as the view is on the stack */
    v->state = UI_VIEW_OPENING;
    v->count = (uint16_t)(v->count + 1);
    if (v->count == 0)
        v->count = 1;
    v->owner = owner;
    v->desc.flags = d->flags;
    v->desc.draw = d->draw;
    v->desc.key = d->key;
    v->desc.closed = d->closed;
    v->desc.arg = d->arg;
    if (ui_defer_active(p->defer)) {
        /* a key callback on a press: the push waits for the release, which our view of the key takes */
        if (ui_defer_add(p->defer, owner, push_deferred, p, slot) != 0) {
            v->state = UI_VIEW_FREE;
            clear_desc(v);
            return 0;
        }
        return handle_of(v, slot);
    }
    if (!push_slot(p, slot))
        return 0;
    return handle_of(v, slot);
}

static void request_close(ui_views *p, ui_view *v)
{
    v->state = UI_VIEW_CLOSING;
    p->fw->close(p->fw->fw, v->fw_view);
}

static void push_deferred(void *ctx, uint32_t slot)
{
    ui_views *p = ctx;
    ui_view *v = &p->v[slot];
    void (*closed)(void *) = v->desc.closed;
    void *arg = v->desc.arg;
    if (v->state != UI_VIEW_OPENING)
        return;
    if (!push_slot(p, slot) && closed)
        closed(arg); /* the caller has the handle: it learns that the view did not come */
}

/* arg: the slot, and the reuse count of the slot in the bits above, so that a close for a view that is gone does not reach the next view in its slot. */
static void close_deferred(void *ctx, uint32_t arg)
{
    ui_views *p = ctx;
    uint32_t slot = arg & 0xff;
    ui_view *v = &p->v[slot];
    if (v->state == UI_VIEW_OPEN && v->count == (arg >> 8))
        request_close(p, v);
}

void ui_views_close(ui_views *p, uintptr_t handle)
{
    uint32_t slot = (uint32_t)(handle & ((1u << HANDLE_BITS) - 1)) - 1;
    ui_view *v;
    if ((handle & ((1u << HANDLE_BITS) - 1)) == 0 || slot >= DK_UI_VIEWS)
        return;
    v = &p->v[slot];
    if (handle != handle_of(v, slot))
        return;
    if (v->state == UI_VIEW_OPENING) {
        /* closed before the push: it never reached the stack, the module still hears `closed` */
        void (*closed)(void *) = v->desc.closed;
        void *arg = v->desc.arg;
        ui_defer_cancel(p->defer, push_deferred, p, slot);
        v->state = UI_VIEW_FREE;
        v->owner = 0;
        clear_desc(v);
        if (closed)
            closed(arg);
        return;
    }
    if (v->state != UI_VIEW_OPEN)
        return;
    if (ui_defer_active(p->defer) &&
        ui_defer_add(p->defer, v->owner, close_deferred, p, slot | ((uint32_t)v->count << 8)) == 0)
        return; /* the view stays until the key is released and takes the release itself */
    request_close(p, v);
}

uint32_t ui_views_close_owner(ui_views *p, uintptr_t owner)
{
    uint32_t n = 0;
    for (uint32_t i = 0; i < DK_UI_VIEWS; i++) {
        ui_view *v = &p->v[i];
        if (v->state == UI_VIEW_FREE || v->owner != owner)
            continue;
        drop_callbacks(v); /* nothing of the desc is called any more */
        if (v->state == UI_VIEW_OPENING) {
            /* its push has not happened: nothing is on the stack, the slot is free */
            ui_defer_cancel(p->defer, push_deferred, p, i);
            v->state = UI_VIEW_FREE;
            v->owner = 0;
            clear_desc(v);
            continue;
        }
        if (v->state == UI_VIEW_OPEN) {
            request_close(p, v);
            n++;
        }
    }
    return n;
}

void ui_views_draw(ui_views *p, uint32_t slot, const dk_canvas *c)
{
    ui_view *v;
    if (slot >= DK_UI_VIEWS)
        return;
    v = &p->v[slot];
    if (v->state == UI_VIEW_OPEN && v->desc.draw)
        v->desc.draw(v->desc.arg, c);
}

int ui_views_key(ui_views *p, uint32_t slot, uint32_t code, uint32_t flags)
{
    ui_view *v;
    dk_key key;
    if (slot >= DK_UI_VIEWS)
        return 0;
    v = &p->v[slot];
    if (v->state == UI_VIEW_FREE || v->state == UI_VIEW_OPENING ||
        (v->desc.flags & DK_UI_VIEW_OVERLAY))
        return 0;
    /* the release of a key whose press queued an action: this view takes it, whatever the module says, so it reaches no view below and none that the action opens */
    if (!(flags & DK_UI_KEY_DOWN) && ui_defer_released(p->defer, code))
        return 1;
    if (v->state == UI_VIEW_CLOSING || !v->desc.key)
        return 1;
    key = ui_key_of_code(code);
    if (key == DK_KEY_NONE)
        return 1; /* a full-screen view covers the screen: a key it cannot name must not act on the stock screen below */
    if (flags & DK_UI_KEY_DOWN) {
        int used;
        ui_defer_enter(p->defer,
                       code); /* what the callback opens or closes waits for the release */
        used = v->desc.key(v->desc.arg, key, flags);
        ui_defer_leave(p->defer);
        return used ? 1 : 0;
    }
    return v->desc.key(v->desc.arg, key, flags) ? 1 : 0;
}

void ui_views_gone(ui_views *p, uint32_t slot)
{
    ui_view *v;
    void (*closed)(void *);
    void *arg;
    if (slot >= DK_UI_VIEWS)
        return;
    v = &p->v[slot];
    if (v->state == UI_VIEW_FREE)
        return;
    closed = v->desc.closed;
    arg = v->desc.arg;
    p->fw->emit(p->fw->fw, DK_LOG_VIEW_CLOSED, DK_LOG_F_KEEP, v->owner, slot, 0);
    /* free first: `closed` may open a view and get this slot */
    v->state = UI_VIEW_FREE;
    v->fw_view = 0;
    v->owner = 0;
    clear_desc(v);
    if (closed)
        closed(arg);
}

void ui_views_poll(const ui_views *p)
{
    for (uint32_t i = 0; i < DK_UI_VIEWS; i++)
        if (p->v[i].state == UI_VIEW_OPEN)
            p->fw->invalidate(p->fw->fw, p->v[i].fw_view);
}

uint32_t ui_views_used(const ui_views *p)
{
    uint32_t n = 0;
    for (uint32_t i = 0; i < DK_UI_VIEWS; i++)
        n += p->v[i].state != UI_VIEW_FREE;
    return n;
}
