/* Host test of the view pool of core/ui (src/view_pool.h, the same source ui.c links) over a fake of the firmware side (`ui_view_fw`). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/view_pool.h"

#define TAGS 8

static ui_views pool;
static ui_defer defer;
static int defer_starts, key_is_down;
static int
    key_mode; /* what the key callback does on a press: 0 nothing, 1 open a view (tag 5), 2 close the view `self_handle` */
static uintptr_t self_handle, opened_handle;
static ui_view_fw fake;
static int draws[TAGS], keys[TAGS], closed[TAGS];
static uint32_t last_key[TAGS], last_flags[TAGS];
static int key_result;
static int pushes, closes, invalidates, push_fails, poll_fails, poll_starts;
static uint32_t pushed_overlay;
static uint32_t close_log[16];
static int close_n;
static int reopen_tag = -1;
static uintptr_t reopened;
static uint32_t last_canvas_w;
/* the log events the pool asked for: (kind, owner, a, b) in order */
static uint32_t ev[16][5];
static int ev_n;

/* Panel codes 100 and 101 have names (1 and 2); every other code has none. */
dk_key ui_key_of_code(uint32_t code)
{
    return code == 100 ? 1 : code == 101 ? 2 : DK_KEY_NONE;
}

static int32_t defer_start(void *ctx)
{
    (void)ctx;
    defer_starts++;
    return 0;
}

static int32_t f_poll_start(void *fw)
{
    (void)fw;
    poll_starts++;
    return poll_fails ? -8 : 0;
}

static uint32_t f_push(void *fw, uint32_t slot, int overlay)
{
    (void)fw;
    pushes++;
    if (push_fails)
        return 0;
    if (overlay)
        pushed_overlay |= 1u << slot;
    return 0x1000 + slot;
}
static void f_close(void *fw, uint32_t view)
{
    (void)fw;
    closes++;
    if (close_n < 16)
        close_log[close_n++] = view;
}
static void f_emit(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a, uint32_t b)
{
    (void)fw;
    if (ev_n < 16) {
        ev[ev_n][0] = kind;
        ev[ev_n][1] = (uint32_t)owner;
        ev[ev_n][2] = a;
        ev[ev_n][3] = b;
        ev[ev_n][4] = flags;
        ev_n++;
    }
}

static void f_invalidate(void *fw, uint32_t view)
{
    (void)fw;
    (void)view;
    invalidates++;
}

static void d_draw(void *arg, const dk_canvas *c)
{
    draws[(intptr_t)arg]++;
    last_canvas_w = c->width;
}
static void d_closed(void *arg);
static void d_draw(void *arg, const dk_canvas *c);
static int32_t d_key(void *arg, dk_key key, uint32_t flags)
{
    int t = (int)(intptr_t)arg;
    keys[t]++;
    last_key[t] = key;
    last_flags[t] = flags;
    if (flags & DK_UI_KEY_DOWN) {
        if (key_mode == 1) {
            dk_ui_view_desc nd = {0, d_draw, 0, d_closed, (void *)(intptr_t)5};
            opened_handle = ui_views_open(&pool, 1, &nd);
        } else if (key_mode == 2) {
            ui_views_close(&pool, self_handle);
        }
    }
    return key_result;
}
static dk_ui_view_desc reopen_desc;
static void d_closed(void *arg)
{
    int t = (int)(intptr_t)arg;
    closed[t]++;
    if (t == reopen_tag)
        reopened = ui_views_open(&pool, 1, &reopen_desc);
}

void vw_reset(void)
{
    memset(draws, 0, sizeof draws);
    memset(keys, 0, sizeof keys);
    memset(closed, 0, sizeof closed);
    key_result = 0;
    pushes = closes = invalidates = push_fails = poll_fails = poll_starts = 0;
    pushed_overlay = 0;
    close_n = 0;
    reopen_tag = -1;
    reopened = 0;
    fake.fw = 0;
    fake.poll_start = f_poll_start;
    fake.push = f_push;
    fake.close = f_close;
    fake.invalidate = f_invalidate;
    fake.emit = f_emit;
    ev_n = 0;
    memset(&pool, 0, sizeof pool);
    memset(&defer, 0, sizeof defer);
    defer.start = defer_start;
    defer_starts = key_is_down = key_mode = 0;
    self_handle = opened_handle = 0;
    ui_views_init(&pool, &fake, &defer);
}

/* with_key: the desc has a key function; with_closed: a closed function. tag identifies the view in the counters. */
uintptr_t vw_open(uintptr_t owner, uint32_t flags, int tag, int with_key, int with_closed)
{
    dk_ui_view_desc d = {flags, d_draw, with_key ? d_key : 0, with_closed ? d_closed : 0,
                         (void *)(intptr_t)tag};
    return ui_views_open(&pool, owner, &d);
}
uintptr_t vw_open_no_draw(void)
{
    dk_ui_view_desc d = {0, 0, 0, 0, 0};
    return ui_views_open(&pool, 1, &d);
}
uintptr_t vw_open_null(void)
{
    return ui_views_open(&pool, 1, 0);
}
void vw_set_closed_reopens(int tag, int reopen_with_tag)
{
    reopen_tag = tag;
    reopen_desc = (dk_ui_view_desc){0, d_draw, 0, d_closed, (void *)(intptr_t)reopen_with_tag};
}
uintptr_t vw_reopened(void)
{
    return reopened;
}
void vw_close(uintptr_t h)
{
    ui_views_close(&pool, h);
}
uint32_t vw_close_owner(uintptr_t owner)
{
    return ui_views_close_owner(&pool, owner);
}
void vw_gone(uint32_t slot)
{
    ui_views_gone(&pool, slot);
}
void vw_draw(uint32_t slot)
{
    dk_canvas c = {128, 64, 0, 0, 0, 0, 0};
    ui_views_draw(&pool, slot, &c);
}
int vw_key(uint32_t slot, uint32_t code, uint32_t flags)
{
    return ui_views_key(&pool, slot, code, flags);
}
void vw_poll(void)
{
    ui_views_poll(&pool);
}
void vw_set_key_result(int r)
{
    key_result = r;
}
/* the key callback of a full-screen view: mode 1 opens a view, mode 2 closes `self_handle` */
void vw_set_key_mode(int mode)
{
    key_mode = mode;
}
void vw_set_self(uintptr_t h)
{
    self_handle = h;
}
uintptr_t vw_opened(void)
{
    return opened_handle;
}
static int is_down(void *k, uint32_t code)
{
    (void)k;
    (void)code;
    return key_is_down;
}
void vw_set_key_down(int on)
{
    key_is_down = on;
}
void vw_defer_poll(void)
{
    ui_defer_poll(&defer, is_down, 0);
}
uint32_t vw_defer_pending(void)
{
    return ui_defer_pending(&defer);
}
void vw_fail_poll(int on)
{
    poll_fails = on;
}
int vw_poll_starts(void)
{
    return poll_starts;
}
void vw_fail_push(int on)
{
    push_fails = on;
}
uint32_t vw_used(void)
{
    return ui_views_used(&pool);
}
int vw_count(int what, int tag)
{
    switch (what) {
    case 0:
        return draws[tag];
    case 1:
        return keys[tag];
    case 2:
        return closed[tag];
    case 3:
        return pushes;
    case 4:
        return closes;
    case 5:
        return invalidates;
    default:
        return (int)last_key[tag];
    }
}
uint32_t vw_last_flags(int tag)
{
    return last_flags[tag];
}
uint32_t vw_pushed_overlay(void)
{
    return pushed_overlay;
}
uint32_t vw_closed_view(int i)
{
    return close_log[i];
}
int vw_events(void)
{
    return ev_n;
}
uint32_t vw_event(int i, int field)
{
    return ev[i][field];
}
uint32_t vw_canvas_width(void)
{
    return last_canvas_w;
}
