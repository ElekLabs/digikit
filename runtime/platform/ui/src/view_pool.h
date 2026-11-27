/* The pool of module screens (dk_ui_view_open of <dk/platform/ui.h>): one owner object for the state of every open view and for the rules of its
   life. It holds no firmware: everything that touches a firmware View goes through `ui_view_fw`, which ui.c implements and the host test fakes
   (runtime/platform/ui/host/tests/views.rs).

   Life of a slot: FREE, then OPEN (the firmware view is on the stack; OPENING first when a key callback opened it: the push waits for the key release, see
   key_defer.h), then CLOSING (a close was asked, the firmware has not removed the view yet: it only
   flags it and a sweep removes it later), then FREE again when the firmware reports the view gone (`ui_views_gone`). A slot is not reused before that,
   because the firmware may still call draw or key of the view until the sweep.
   A handle is the slot number plus a count that changes at every reuse, so a handle that is closed already, or one that a unload closed, does nothing.
   The firmware calls `ui_views_draw`, `ui_views_key` and `ui_views_gone` with the slot number it was given at `push`. */
#ifndef DK_UI_VIEW_POOL_H
#define DK_UI_VIEW_POOL_H
#include <stdint.h>
#include <dk/platform/ui.h>
#include <dk/gen/keys.h>
#include <dk/gen/log_kinds.h>
#include "key_defer.h"

/* What the pool asks of the firmware. `fw` is passed back to each call. */
typedef struct ui_view_fw {
    void *fw;
    /* Called before anything is taken for a new view: make sure the per-pass poll runs (ui_views_poll is called from it). 0, or a negative code: the view is then
       not opened, so `closed` of its desc never runs for a view the caller did not get. */
    int32_t (*poll_start)(void *fw);
    /* Make the firmware view for slot `slot` (it calls ui_views_draw, ui_views_key and ui_views_gone with it), push it on the view stack and give
       back our own reference to it. Returns the view as an opaque word, or 0 when it could not be made. `overlay`: not opaque, keys pass on. */
    uint32_t (*push)(void *fw, uint32_t slot, int overlay);
    /* Flag the view for removal; the firmware reports it with ui_views_gone later. */
    void (*close)(void *fw, uint32_t view);
    /* Ask for a redraw of the view. */
    void (*invalidate)(void *fw, uint32_t view);
    /* A log event (`DK_LOG_VIEW_OPENED` or `DK_LOG_VIEW_CLOSED` of <dk/gen/log_kinds.h>) with its flags (always `DK_LOG_F_KEEP`), the owner handle and two words. */
    void (*emit)(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a, uint32_t b);
} ui_view_fw;

enum { UI_VIEW_FREE, UI_VIEW_OPENING, UI_VIEW_OPEN, UI_VIEW_CLOSING };

typedef struct ui_view {
    uint8_t state;
    uint16_t count; /* changes at each open of this slot; part of the handle */
    uintptr_t owner;
    uint32_t fw_view;     /* the firmware view, opaque */
    dk_ui_view_desc desc; /* a copy */
} ui_view;

typedef struct ui_views {
    ui_view v[DK_UI_VIEWS];
    const ui_view_fw *fw;
    ui_defer *defer; /* the actions that wait for a key release, shared with the text input */
} ui_views;

void ui_views_init(ui_views *p, const ui_view_fw *fw, ui_defer *defer);

/* Open a view for `owner`. In a key callback (a press) the view is opened when the key is released: the handle is valid at once (`closed` runs if the push then
   fails, or if the handle is closed before the push) and the slot is taken. The handle (never 0), or 0 when the desc is wrong (no desc, no draw, unknown flags) or no slot is free or the firmware
   could not make the view. */
uintptr_t ui_views_open(ui_views *p, uintptr_t owner, const dk_ui_view_desc *d);

/* Ask for the close of an open view; in a key callback (a press) it happens when the key is released. No effect on a handle that is closed or not valid. `closed` of the desc runs later, from ui_views_gone. */
void ui_views_close(ui_views *p, uintptr_t handle);

/* The owner unloads: close each of its views and call nothing of its desc any more (not draw, key or closed). Returns how many were closed. */
uint32_t ui_views_close_owner(ui_views *p, uintptr_t owner);

/* Firmware to pool. draw: the view is on screen this frame. */
void ui_views_draw(ui_views *p, uint32_t slot, const dk_canvas *c);

/* Firmware to pool: a key event of the view. Returns 1 when the key is used and must not go on to the views below, 0 when it passes on. An overlay
   passes every key on. A full-screen view takes every key that it does not pass on itself: a key without a logical name, or a view without `key`, or one
   that is closing, is taken without a call, so no key acts on the stock screen that the view covers. */
int ui_views_key(ui_views *p, uint32_t slot, uint32_t code, uint32_t flags);

/* Firmware to pool: the view left the stack. The slot is free, then `closed` of the desc runs (unless the owner is gone). */
void ui_views_gone(ui_views *p, uint32_t slot);

/* Once per UI pass while any view is open: asks the firmware to redraw each open view, as the firmware draws only after a request. */
void ui_views_poll(const ui_views *p);

/* Views that are OPEN or CLOSING. */
uint32_t ui_views_used(const ui_views *p);

/* The logical key for a panel code, or DK_KEY_NONE for a code that has no name. Defined in panel_keys.c (the host test has its own). */
dk_key ui_key_of_code(uint32_t code);

#endif
