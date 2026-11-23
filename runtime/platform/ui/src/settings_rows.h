/* The SETTINGS rows of modules (dk_ui_settings_row of <dk/platform/ui.h>): a small registry with no firmware and no runtime call, so the host can test
   it (runtime/platform/ui/host/tests/settings_rows.rs). ui.c owns the one registry, fills it from dk_ui_settings_row, empties it for an owner on unload
   and, from the hook at the end of the SETTINGS rebuild, adds one stock row for each entry in load order. A stock row is found again by a key, not by its
   place (a removal moves the rows after it) and not by a number of one load: the key is the name of the owner's record (a module or a service file) and the row's number within the owner, which a
   reload of the module gives again. So after a reload the stock row that was built before runs the reloaded module's row, and after a plain unload (no row with
   that key) it does nothing. The keys live as long as the image: a stock row may be run long after its module left. */
#ifndef DK_UI_SETTINGS_ROWS_H
#define DK_UI_SETTINGS_ROWS_H
#include <stdint.h>
#include <dk/platform/ui.h>

#define DK_UI_MODULE_NAME 32 /* a module name with its NUL (DkrtModule.name) */

/* What a stock row holds to find its row again: the module's name and the row's number within the module (from 0, in the order of its calls). Never freed. */
typedef struct ui_row_key {
    char name[DK_UI_MODULE_NAME];
    uint32_t index;
    uint8_t used;
} ui_row_key;

typedef struct ui_settings_row {
    dk_ui_settings_open_fn *open; /* 0: not used (never in the first `n` entries) */
    void *arg;
    uintptr_t owner;
    const ui_row_key *key;
    char text[DK_UI_ROW];
} ui_settings_row;

typedef struct ui_settings_rows {
    ui_settings_row row[DK_UI_SETTINGS_ROWS];
    uint32_t n;
    /* the keys of every row that was ever added: room for the rows of several loads of a few modules */
    ui_row_key key[DK_UI_SETTINGS_ROWS * 2];
} ui_settings_rows;

/* Add a row at the end. `name`: the name of the owner's record, a module or a service file (at most DK_UI_MODULE_NAME - 1 characters are kept); the row's
   number within the owner is the count of the owner's rows now in the registry. `in_init`: the caller is the owner's `init` (the only place a row may be
   added). DK_OK, DK_E_CONTEXT (not in init), DK_E_CONTRACT (no text or open, or no name: owners without one would share a key) or DK_E_NOMEM (all rows used, or no room for a new key). The text is copied and cut to DK_UI_ROW - 1. */
int32_t ui_rows_add(ui_settings_rows *s, uintptr_t owner, const char *name, int in_init,
                    const char *text, dk_ui_settings_open_fn *open, void *arg);

/* Remove every row of `owner`; the rows after them keep their order. Returns how many were removed. */
uint32_t ui_rows_remove_owner(ui_settings_rows *s, uintptr_t owner);

/* Call `open(arg)` of the row that now has this key. 1 when the row is there (also a row of a reloaded module), 0 when there is none (its module is unloaded). */
int ui_rows_run(const ui_settings_rows *s, const ui_row_key *key);

#endif
