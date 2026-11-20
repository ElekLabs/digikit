/* The pool of module screens (view_pool.h). Pure logic over the `ui_view_fw` interface. */
#include "view_pool.h"

#define HANDLE_BITS 8u

static uintptr_t handle_of(const ui_view *v, uint32_t slot)
{
    return ((uintptr_t)v->count << HANDLE_BITS) | (slot + 1);
}

/* The slot a handle names, or 0 when it is not valid or not OPEN. */
static ui_view *open_slot(ui_views *p, uintptr_t handle)
{
    uint32_t slot = (uint32_t)(handle & ((1u << HANDLE_BITS) - 1)) - 1;
    ui_view *v;
    if ((handle & ((1u << HANDLE_BITS) - 1)) == 0 || slot >= DK_UI_VIEWS)
        return 0;
    v = &p->v[slot];
    if (v->state != UI_VIEW_OPEN || handle != handle_of(v, slot))
        return 0;
    return v;
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

void ui_views_init(ui_views *p, const ui_view_fw *fw)
{
    p->fw = fw;
    for (uint32_t i = 0; i < DK_UI_VIEWS; i++) {
        p->v[i].state = UI_VIEW_FREE;
        p->v[i].count = 0;
        p->v[i].owner = 0;
        p->v[i].fw_view = 0;
        clear_desc(&p->v[i]);
    }
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
    /* the slot is taken before the push: the firmware may call back into it as soon as the view is on the stack */
    v->state = UI_VIEW_OPEN;
    v->count = (uint16_t)(v->count + 1);
    if (v->count == 0)
        v->count = 1;
    v->owner = owner;
    v->desc.flags = d->flags;
    v->desc.draw = d->draw;
    v->desc.key = d->key;
    v->desc.closed = d->closed;
    v->desc.arg = d->arg;
    v->fw_view = p->fw->push(p->fw->fw, slot, (d->flags & DK_UI_VIEW_OVERLAY) != 0);
    if (!v->fw_view) {
        v->state = UI_VIEW_FREE;
        clear_desc(v);
        return 0;
    }
    return handle_of(v, slot);
}

static void request_close(ui_views *p, ui_view *v)
{
    v->state = UI_VIEW_CLOSING;
    p->fw->close(p->fw->fw, v->fw_view);
}

void ui_views_close(ui_views *p, uintptr_t handle)
{
    ui_view *v = open_slot(p, handle);
    if (v)
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
    uint32_t key;
    if (slot >= DK_UI_VIEWS)
        return 0;
    v = &p->v[slot];
    if (v->state == UI_VIEW_FREE || (v->desc.flags & DK_UI_VIEW_OVERLAY))
        return 0;
    if (v->state == UI_VIEW_CLOSING || !v->desc.key)
        return 1;
    key = ui_key_of_code(code);
    if (key == UI_KEY_NONE)
        return 1; /* a full-screen view covers the screen: a key it cannot name must not act on the stock screen below */
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
