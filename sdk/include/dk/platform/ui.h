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
   - A content module (a .dkm) has no unload at run time, so no UI object of the module is freed. Every
     arg, callback and descriptor a module passes must still point at its own memory (static data or
     code of the module), never at the stack or at a heap block that is freed later. Build a menu when
     the user opens it (in `menu`, or from a callback), after all modules are loaded.

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

/* Bytes of row text, terminator included. Longer text is cut. */
#define DK_UI_ROW 24
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

/* Make a menu from a descriptor; NULL when out of memory. `desc` must stay valid: it is kept. Context: ui. Since api 1.11. */
dk_ui_menu *dk_ui_menu_new(dk_ui *ui, const dk_ui_menu_desc *desc);

/* A choice of `n` items. `done(arg, index)` gets the index; NO closes the dialog without calling `done`. Returns 0 when shown, a negative code when not
   (out of memory, or another dialog is open). `title` up to 80 characters. Context: ui. Since api 1.11. */
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

#endif
