/* <dk/platform/ui.h>: menus, lists and dialogs on the device screen. Hand-written.
   The calls are device-agnostic. core/ui implements them for each device with the firmware's own views. The prototypes of the generated calls are in
   <dk/gen/imports.h> (included here); this header adds the menu descriptor, the flags and the two calls that need typed pointers.

   `dk_ui_open(ctx, &ui)` in `init` gives the UI handle, or DK_E_MISSING when core/ui is not in the image: the module then loads and decides whether
   it can run without menus. The calls that make an object or a dialog take the handle first: dk_ui_menu_new, dk_ui_confirm, dk_ui_choice,
   dk_ui_toast and dk_ui_tick. The calls that work on a menu take the menu: dk_ui_menu_open/close/select/refresh/is_open, the rows
   dk_ui_menu_add_text/add_action/add_submenu/add_toggle/add_value and dk_ui_menu_check/checked. An action or change callback is a
   `dk_ui_action_fn`: `void (void *arg, int32_t value)`; `dk_ui_menu_add_action(menu, text, run, arg, value)` gives it the pointer `arg` and the `value`
   the row was added with. The module's settings menu is the `menu` field of `dk_module`: `void menu(dk_module_ctx *ctx, dk_ui_menu *menu)`.

   A menu is built once and kept. Its fill function adds the rows each time the menu opens or is refreshed:

     static void fill(dk_ui_menu *m, void *arg) {
         dk_ui_menu_add_action(m, "APPLY", apply, arg, 0);
         dk_ui_menu_add_toggle(m, "LOUD", &loud, loud_changed, arg);
     }
     static const dk_ui_menu_desc main_desc = {.name = "MyMenu", .title = "MY MODULE", .fill = fill};
     dk_ui_menu_open(dk_ui_menu_new(ui, &main_desc));

   Lifetime rules:
   - Text is copied when a row is added or a dialog opens. The caller may reuse its buffers.
   - `arg` is kept and passed to the callbacks later, after the opening function has returned.
     It must point at static or module memory, never at the stack.
   - Callbacks run on the UI task. A dialog's callback runs after the dialog has closed.
   - Menus are never freed. Dialogs free themselves after they close.
   - A menu is never freed and keeps its descriptor, `fill` and `arg`. A module that built a menu with `dk_ui_menu_new` (or whose settings menu the DIGIKIT menu
     has opened) is therefore not unloaded or reloaded while the device runs: the runtime refuses with DK_E_RELOAD_MENUS until the next restart. A module that builds
     none can be. Every arg, callback and descriptor a module passes must still point at its own memory (static data or code of the module), never at the stack or at a heap block
     that is freed later. Build a menu when the user opens it (in `menu`, or from a callback), after all modules are loaded.

   A module with settings sets the `menu` field of `dk_module`. The DIGIKIT menu shows SETTINGS... for the loaded module and calls it only when the user
   opens it; it adds the rows of that menu:

     static void menu(dk_module_ctx *ctx, dk_ui_menu *m) { dk_ui_menu_add_action(m, "RESET", reset, 0, 0); }

   The calls are imports of the module (sdk/sdk.toml, requires core/ui). When core/ui is not in the
   image the module still loads: `dk_ui_open` returns DK_E_MISSING, and the other calls return a failure value
   (dk_ui_menu_new NULL, the calls with a result a negative code, dk_ui_menu_checked and dk_ui_menu_is_open 0) or do nothing.

   Keys: YES runs a row's action, flips a toggle, or (in a list with a check function) toggles the
   row's check mark. The encoder changes a value row; changed is called only when the value moves.
   In a list, RIGHT opens the side panel; choosing a panel row closes the panel, then runs the row.
   LEFT or NO closes the panel. NO on a menu goes back. */
#ifndef DK_UI_H
#define DK_UI_H
#include <stdint.h>
#include <dk/gen/imports.h>
#include <dk/platform/canvas.h>
#include <dk/gen/keys.h>

/* Label limits include the terminator. Plain text rows in a list scroll when selected. */
#define DK_UI_ROW 24
#define DK_UI_LIST_TEXT 80
/* Characters a toast shows. */
#define DK_UI_TOAST 32

/* Adds the rows of a menu or of a list's side panel. */
typedef void dk_ui_fill_fn(dk_ui_menu *m, void *arg);

/* Menu flags. */
enum {
    DK_UI_TEXT_PAGE = 1, /* rows are text to read: no highlighted row, the keys scroll */
    DK_UI_LIST = 2,      /* a dense list with a title bar, a check column and a side panel */
    DK_UI_WIDE =
        4, /* a menu page with a full-width title bar and full-width rows (a list without check marks or side panel) */
};

typedef struct dk_ui_menu_desc {
    const char *name;  /* view name, for logs */
    const char *title; /* shown in the title */
    uint32_t flags;    /* DK_UI_* */
    dk_ui_fill_fn *fill;
    void *arg; /* passed to fill, panel, check and back */
    /* DK_UI_LIST only. Each may be 0. */
    const char *empty; /* text shown when the list has no rows */
    void (*panel)(dk_ui_menu *panel, int32_t row,
                  void *arg);                          /* RIGHT on row: add the side-panel rows */
    void (*check)(void *arg, int32_t row, int32_t on); /* YES on row toggled its check mark */
    int32_t (*back)(void *arg);                        /* NO on the list: return 1 to stay open */
} dk_ui_menu_desc;

/* Make a menu from a descriptor; NULL when out of memory. `desc` must stay valid: it is kept. Context: ui. Since api 2.11. */
dk_ui_menu *dk_ui_menu_new(dk_ui *ui, const dk_ui_menu_desc *desc);

/* A choice of `n` items. `done(arg, index)` gets the index; NO closes the dialog without calling `done`. Returns 0 when shown, a negative code when not
   (out of memory, or another dialog is open). `title` up to 80 characters. Context: ui. Since api 2.11. */
int32_t dk_ui_choice(dk_ui *ui, const char *title, const char *const *items, int32_t n,
                     dk_ui_choice_fn *done, void *arg);

/* The other dialogs are in <dk/gen/imports.h>:
     int32_t dk_ui_confirm(ui, title, text, done, arg)   YES or NO; done(arg, yes) gets 1 for YES, 0 for NO. title and text: up to 80 characters each.
     int32_t dk_ui_toast(ui, text)                       a short message on the main screen that goes away by itself.
   Rows are added from a fill function. In a list, the row number (from 0, in the order added) is the `row` argument of panel and check;
   dk_ui_menu_select puts the cursor on a row before dk_ui_menu_open, so a list starts on its current choice (dk_ui_menu_open keeps the cursor row;
   a text page opens at its first row). dk_ui_menu_refresh fills again; the selection stays on the same row when it can. dk_ui_menu_add_toggle: ON/OFF,
   `*value` is read when drawn, `changed` gets the new value after `*value` is written. dk_ui_menu_add_value: a number from `min` to `max` changed by the
   encoder. dk_ui_menu_check / dk_ui_menu_checked: the check marks of a DK_UI_LIST.

   A timer in the UI task, `dk_ui_tick(ui, fn, arg, period_ms)`: fn(arg) runs about every period_ms, in the same task as the fill and action callbacks,
   so it may call dk_ui_menu_refresh and read module state without a lock. Use it to redraw a screen that shows a change nobody triggers with a key
   (a load that is still running). Rules:
   - The accuracy is the UI loop (about 8 ms) at best; a callback runs late when the task is busy. Do not count calls: read the state.
   - Keep fn short. It runs between key events and delays them. It must not wait.
   - fn and arg stay in use: static module code and memory, never the stack.
   - A timer is identified by (fn, arg). Calling again with the same pair changes its period (the next call is one period away).
     period_ms 0 stops it, also from inside fn; stopping a timer that does not run is not an error. Stop the timer when its screen
     closes: a running timer costs a call per period.
   - Room for 4 timers in the image, shared by all modules. A longer period than 1,000,000 ms is cut.
   Returns 0 when set or stopped, a negative code when it could not be (fn 0, or the table is full). */

/* ---- Module screens ----
   `dk_ui_view_open(ui, &desc)` opens a screen the module draws. Context: ui. Returns a handle, or NULL when the desc is wrong or the pool is full (room for
   DK_UI_VIEWS views in the image, shared by all modules).
   - DK_UI_VIEW_OVERLAY: the view is drawn over the stock screen, and every key goes on to the stock views (`key` is not called). Without the flag the view
     covers the screen and gets the keys first: `key` returns 1 when it used the key, and 0 to pass it on to the stock view below.
   - `draw(arg, canvas)` runs for every frame while the view is open (<dk/platform/canvas.h>).
   - `closed(arg)` runs once, after the view left the screen: after `dk_ui_view_close`, or when the system closed it. It may open a view again.
   - `dk_ui_view_close` with the handle closes the view. Closing a handle twice, or after `closed`, does nothing. A view is closed for you when its module unloads
     (without a `closed` call, as the module is gone), so a module with views stays unloadable.
   - `desc` is copied. `arg` is kept: static or module memory, never the stack. Callbacks run in the UI task and must be short.
   `key(arg, key, flags)` gets the logical key (<dk/gen/keys.h>) and the event flags: DK_UI_KEY_DOWN is set for a press and clear for a release; the other bits
   are reserved. A key the device gives no name is not passed to the view and does not act on the stock screen. */
#define DK_UI_KEY_DOWN 1u
enum {
    DK_UI_VIEW_OVERLAY = 1,
};
#define DK_UI_VIEWS 4

typedef struct dk_ui_view_desc {
    uint32_t flags; /* DK_UI_VIEW_* */
    void (*draw)(void *arg, const dk_canvas *canvas);
    int32_t (*key)(void *arg, dk_key key, uint32_t flags); /* full screen only; 1 = used */
    void (*closed)(void *arg);                             /* 0 or a function */
    void *arg;
} dk_ui_view_desc;

dk_ui_view *dk_ui_view_open(dk_ui *ui, const dk_ui_view_desc *desc);

/* ---- A row in SETTINGS ----
   `dk_ui_settings_row(ui, text, open, arg)`, in `init` only: adds a row `text` (up to DK_UI_ROW - 1 characters) at the end of the SETTINGS list.
   `open(arg)` runs on YES, in the UI task. Rows are in load order, after the rows of the DIGIKIT menu. Room for DK_UI_SETTINGS_ROWS rows in the image.
   The row goes away when the module unloads. Returns 0, DK_E_CONTRACT (no `text` or `open`) or DK_E_NOMEM (no room). */
#define DK_UI_SETTINGS_ROWS 4

/* ---- The stock naming screen ----
   `dk_ui_text_input(ui, &desc)` opens the firmware's own text entry. `done(arg, text)` runs on YES only, with the text as edited (trailing blanks
   removed); NO closes the screen and calls nothing. `title` is the prompt, `initial` the text to start from (NULL: empty; longer than `max_len`: cut).
   `max_len` is 1 to DK_UI_TEXT_INPUT_MAX and must fit the screen. One entry screen at a time. Context: ui.
   Returns 0 when shown, DK_E_CONTRACT (the desc is wrong) or DK_E_BUSY (a text input is open) or another negative code. */
#define DK_UI_TEXT_INPUT_MAX 15

typedef struct dk_ui_text_input_desc {
    const char *title, *initial;
    uint32_t max_len;
    void (*done)(void *arg, const char *text);
    void *arg;
} dk_ui_text_input_desc;

int32_t dk_ui_text_input(dk_ui *ui, const dk_ui_text_input_desc *desc);

#endif
