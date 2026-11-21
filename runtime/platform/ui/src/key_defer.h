/* Actions of a module's key callback that wait for the key to be released (no firmware, so the host can test it: host/tests/defer.rs).
   The stock views act on the release of YES and NO (the event flags have bit 4 set and bit 0 clear), and the release event goes to the view that is on top when it
   comes. A module key callback runs on the press. If it opened or closed a view, or started a text input, at once, the release would reach the new top view (a NameView
   would take it as YES) or the view below the closed one (SETTINGS would go back too). So these actions are queued here with the key code of the press, the module's
   view stays on top and takes the release (`ui_defer_released`), and the per-pass poll runs the actions after it. Should the release event never come, the key state
   ends the wait after a few passes. */
#ifndef DK_UI_KEY_DEFER_H
#define DK_UI_KEY_DEFER_H
#include <stdint.h>

#define UI_DEFER_MAX 4u
/* Polls with the key up and no release event seen before an action runs anyway. */
#define UI_DEFER_IDLE_POLLS 3u

typedef struct ui_defer_item {
    void (*run)(void *ctx, uint32_t arg);
    void *ctx;
    uint32_t arg;
    uintptr_t owner;
    uint32_t code; /* the key whose release it waits for */
    uint8_t used;
    uint8_t released; /* the release event came to the view */
    uint8_t idle;     /* polls with the key up */
} ui_defer_item;

typedef struct ui_defer {
    ui_defer_item item[UI_DEFER_MAX];
    uint32_t code; /* the key of the press callback that runs now */
    uint8_t in_key;
    /* Makes the per-pass poll run (it calls ui_defer_poll). 0, or a negative code: the action is then refused. */
    int32_t (*start)(void *ctx);
    void *start_ctx;
} ui_defer;

/* Around a module key callback on a press of `code`. */
void ui_defer_enter(ui_defer *d, uint32_t code);
void ui_defer_leave(ui_defer *d);

/* Is a press callback running (an action must wait for the release)? */
int ui_defer_active(const ui_defer *d);

/* Queue `run(ctx, arg)` for the release of the key of the running press callback. DK_OK, or DK_E_NOMEM when the queue is full or the poll cannot start. */
int32_t ui_defer_add(ui_defer *d, uintptr_t owner, void (*run)(void *ctx, uint32_t arg), void *ctx,
                     uint32_t arg);

/* A release event of `code` reached the view. 1 when an action waits for it: the view must take the release. */
int ui_defer_released(ui_defer *d, uint32_t code);

/* Once per UI pass: run what is ready (the release was seen, or the key stayed up for UI_DEFER_IDLE_POLLS polls). `down(kctx, code)` is the key state. An action
   may queue another. */
void ui_defer_poll(ui_defer *d, int (*down)(void *kctx, uint32_t code), void *kctx);

/* Remove the queued action `(run, ctx, arg)`; 1 when it was there. */
int ui_defer_cancel(ui_defer *d, void (*run)(void *ctx, uint32_t arg), void *ctx, uint32_t arg);

/* Remove the actions of `owner` (it unloads). */
uint32_t ui_defer_drop_owner(ui_defer *d, uintptr_t owner);

/* How many actions wait. */
uint32_t ui_defer_pending(const ui_defer *d);

#endif
