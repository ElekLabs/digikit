/* <dk/settings/page.h>: a saved settings page from a table of fields (docs/design/ui-rules.md; RC3m). Shared source, not a runtime call: the packer links
   sdk/src/settings/field.c and page.c when the module calls them. It sits on <dk/platform/settings.h> and <dk/platform/ui.h> and declares no import.

   The module declares its fields once. The page fills the menu that DIGIKIT > MODULES > the module > SETTINGS... gives to `dk_module.menu`, so the module
   builds no menu of its own and stays unloadable and reloadable.

     static const char *const MODES[] = {"OFF", "SLOW", "FAST"};
     static const dk_settings_field FIELDS[] = {
         {.key = "loud", .label = "LOUD", .kind = DK_SETTINGS_BOOL, .def = 1},
         {.key = "gain", .label = "GAIN", .kind = DK_SETTINGS_INT, .def = 50, .min = 0, .max = 100},
         {.key = "mode", .label = "MODE", .kind = DK_SETTINGS_ENUM, .def = 1, .max = 2, .labels = MODES},
     };
     static int32_t init(dk_module_ctx *ctx) {
         dk_settings *settings; dk_ui *ui;
         if (dk_settings_open(ctx, &settings) != DK_OK || dk_ui_open(ctx, &ui) != DK_OK) return DK_E_MISSING;
         dk_settings_page_desc d = {.fields = FIELDS, .count = 3, .changed = on_change};
         return dk_settings_page_init(&d, settings, ui);
     }
     static void menu(dk_module_ctx *ctx, dk_ui_menu *m) { dk_settings_page_fill(m); }

   Kinds: BOOL (a toggle; min and max are ignored; def is 0 or 1), INT (a value row: min <= def <= max within -99 .. 999) and ENUM (the options `labels[0 .. max]`,
   min is 0, max is 1 .. 31; a row that shows the option, YES opens the stock choice popup, a pick saves). Options may only be appended: the stored number is the index.
   Each field is stored under its `key` as decimal text ("-5", "42"; a BOOL is "0" or "1"). A value that is missing, not a number or out of range reads as `def`.
   The row text is the label (BOOL, INT) or "LABEL OPTION" (ENUM) and must fit DK_UI_ROW - 1 characters; the table is refused otherwise.
   The values load in `dk_settings_page_init` only. A change from outside arrives through the module's reload, as for every settings reader.

   After an edit the page saves the field; `changed(arg, field, value)` runs after that. A failed save puts the saved value back and adds the row NOT SAVED to the
   page. A field with DK_SETTINGS_RESTART adds the row ON NEXT RESTART after a change: its module reads the value at start only.
   One page exists per module (statics). Everything runs in the UI task. */
#ifndef DK_SETTINGS_PAGE_H
#define DK_SETTINGS_PAGE_H
#include <stdint.h>
#include <dk/core/module.h>
#include <dk/core/status.h>
#include <dk/platform/settings.h>
#include <dk/platform/ui.h>

enum { DK_SETTINGS_BOOL, DK_SETTINGS_INT, DK_SETTINGS_ENUM };
enum { DK_SETTINGS_RESTART = 1 }; /* flags: the value applies at the next restart */

#define DK_SETTINGS_FIELDS 12     /* fields per page, at most */
#define DK_SETTINGS_KEY 31        /* characters of a key, at most (the store allows 47 for module name and key together) */

typedef struct dk_settings_field {
    const char *key;   /* printable ASCII without space, "/" and ".." */
    const char *label; /* the row text, upper case */
    uint8_t kind;      /* DK_SETTINGS_BOOL, _INT or _ENUM */
    uint8_t flags;     /* DK_SETTINGS_RESTART or 0 */
    int16_t def, min, max;
    const char *const *labels; /* ENUM: max + 1 option texts; otherwise 0 */
} dk_settings_field;

/* A user edit was saved: `field` is the index in the table, `value` the new value (an ENUM's index). */
typedef void dk_settings_changed_fn(void *arg, uint32_t field, int32_t value);

typedef struct dk_settings_page_desc {
    const dk_settings_field *fields; /* static: kept (the descriptor itself is copied) */
    uint32_t count;                  /* 1 .. DK_SETTINGS_FIELDS */
    dk_settings_changed_fn *changed; /* optional */
    void *arg;                       /* passed to changed */
} dk_settings_page_desc;

/* Call in `init` with the module's `dk_settings` and `dk_ui` handles (`ui` may be 0 for a table without an ENUM). Checks the table (DK_E_FORMAT for a bad
   key, kind, range or label, or a bad count), then loads every value. DK_E_MISSING for a missing handle. DK_OK otherwise. */
int32_t dk_settings_page_init(const dk_settings_page_desc *desc, dk_settings *settings, dk_ui *ui);
/* The rows of the page; call from `dk_module.menu`. Adds a text row for a page that was not initialised. */
void dk_settings_page_fill(dk_ui_menu *menu);
/* The value of a field: the loaded or last saved one (0 for an index outside the table or before init). */
int32_t dk_settings_page_value(uint32_t field);
/* Sets a field from the module's own code (UI task): clamps to the range, saves, and shows it on the open page. `changed` does not run. DK_OK, DK_E_NOT_FOUND for an
   index outside the table, or the failed save's code (the value stays as it was). */
int32_t dk_settings_page_set(uint32_t field, int32_t value);
#endif
