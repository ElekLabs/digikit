/* The SETTINGS rows of modules (dk_ui_settings_row of <dk/platform/ui.h>): a small registry with no firmware and no runtime call, so the host can test
   it (modules/core/adapters/ui/host/tests/settings_rows.rs). ui.c owns the one registry, fills it from dk_ui_settings_row, empties it for an owner on unload
   and, from the hook at the end of the SETTINGS rebuild, adds one stock row for each entry in load order. A row is found again by its id, not by its
   place, because a removal moves the rows after it. */
#ifndef DK_UI_SETTINGS_ROWS_H
#define DK_UI_SETTINGS_ROWS_H
#include <stdint.h>
#include <dk/platform/ui.h>

typedef struct ui_settings_row {
    dk_ui_settings_open_fn *open; /* 0: not used (never in the first `n` entries) */
    void *arg;
    uintptr_t owner;
    uint32_t id; /* never 0, never reused until the counter wraps */
    char text[DK_UI_ROW];
} ui_settings_row;

typedef struct ui_settings_rows {
    ui_settings_row row[DK_UI_SETTINGS_ROWS];
    uint32_t n;
    uint32_t last_id;
} ui_settings_rows;

/* Add a row at the end. DK_OK, DK_E_CONTRACT (no text or open) or DK_E_NOMEM (all rows used). The text is copied and cut to DK_UI_ROW - 1. */
int32_t ui_rows_add(ui_settings_rows *s, uintptr_t owner, const char *text,
                    dk_ui_settings_open_fn *open, void *arg);

/* Remove every row of `owner`; the rows after them keep their order. Returns how many were removed. */
uint32_t ui_rows_remove_owner(ui_settings_rows *s, uintptr_t owner);

/* Call `open(arg)` of the row with this id. 1 when the row is there, 0 when it is gone (removed after its stock row was built). */
int ui_rows_run(const ui_settings_rows *s, uint32_t id);

#endif
