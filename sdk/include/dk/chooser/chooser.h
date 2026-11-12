/* <dk/chooser/chooser.h>: the slot list, a chooser for a module with numbered slots that files are loaded into (docs/design/ui-rules.md, "The
   chooser component"). Shared source, not a runtime call: the packer links sdk/src/chooser/chooser.c, logic.c and the core text and path
   sources when the module calls them. It sits on <dk/platform/ui.h> (core/ui lists, the firmware font) and the listing of <dk/platform/files.h>.

   The flow (every slot is a row, an empty slot too):
     list of slots   YES on a slot opens the file browser for that slot (a filled slot is replaced);
                     NO closes the list; RIGHT opens the side panel (panel callback);
     file browser    YES on a file calls fill(arg, slot, path), then the whole flow closes and done(arg, slot) runs;
                     NO goes up one folder, out of the browser at the root, back to the list of slots.

   The rules it keeps (ui-rules.md):
     - the list opens with the current slot marked and the cursor on it (the row callback draws the mark, dk_ui_menu_select puts the cursor);
     - YES on a file finishes the flow: every level closes in one step, then done runs, on the page the flow started from;
     - NO goes back one level and changes nothing;
     - LEFT or NO closes the side panel;
     - an error never opens a dialog: fill's error closes the flow and shows its text as a toast on the page; done is not called;
     - while busy() says rows change by themselves, the list refreshes on dk_ui_tick.

   One chooser exists at a time (statics); a second open reuses it. Its menus are built on the first open, so the title of the first open stays.

     init:       dk_files_open(ctx, &files); dk_ui_open(ctx, &ui); dk_chooser_init(files, ui);
     click:      dk_chooser_open(&desc, current);     desc is static (kept)
     fini:       dk_chooser_fini();

   The chooser keeps the two handles (it takes no module context: the module opens them in its own `init`). Everything runs in the UI task. dk_chooser_open
   returns a negative DK_E_* code when init was not called or failed, and does nothing then. */
#ifndef DK_CHOOSER_H
#define DK_CHOOSER_H
#include <stdint.h>
#include <dk/core/module.h>
#include <dk/core/status.h>
#include <dk/platform/files.h>
#include <dk/platform/ui.h>

#define DK_CHOOSER_E_PATH                                                                          \
    DK_E_SEL_PATH /* the browser's path does not fit 255 bytes: fill is not called, the flow fails with this code */

typedef struct dk_chooser_desc {
    const char *title; /* list title */
    int32_t count;     /* slots 0 .. count-1 */
    /* Writes the text of slot i (at most DK_UI_ROW - 1 characters). is_current: the slot is the current one; mark it in the text. An
       empty slot gets a row too (for example "-"). */
    void (*row)(void *arg, int32_t i, int32_t is_current, char *text);
    int32_t (*busy)(
        void *
            arg); /* optional: 1 while any row changes without a key (a load); the list then refreshes every 250 ms */
    uint32_t root; /* DK_FILES_* root of the browser */
    /* YES on a file (root-relative path, at most 255 bytes) for `slot`: the module loads it there. Returns the slot for done (>= 0), or a
       negative error. Required. */
    int32_t (*fill)(void *arg, int32_t slot, const char *path);
    /* The text of an error (at most 31 characters) for the toast; optional (the default is "ERROR -n"). */
    void (*error_text)(void *arg, int32_t code, char *text);
    /* RIGHT on slot i: add side-panel rows with dk_ui_menu_add_action. Optional. */
    void (*panel)(void *arg, dk_ui_menu *panel, int32_t slot);
    /* The flow finished: the slot fill returned. Every level is closed. Optional. */
    void (*done)(void *arg, int32_t slot);
    void *arg; /* passed to every callback above */
} dk_chooser_desc;

/* Call in `init` with the module's `dk_files` and `dk_ui` handles (dk_files_open, dk_ui_open); the chooser keeps both. DK_OK, or DK_E_MISSING for a NULL handle
   (open then does nothing). */
int32_t dk_chooser_init(dk_files *files, dk_ui *ui);
/* Opens the list. current is the entry to mark (-1: none); the cursor starts on it. DK_OK when opened, a negative DK_E_* code when not. Call from the UI task. */
int32_t dk_chooser_open(const dk_chooser_desc *d, int32_t current);
/* Fills the open list again (a panel action changed a row). */
void dk_chooser_refresh(void);
/* Text rows of at most 16 characters, broken at a space or after a '/': for a module's own text page. */
void dk_chooser_wrap(dk_ui_menu *m, const char *s);
/* Stops the timer and the listing, and lets go of the handles; call in `fini`. */
void dk_chooser_fini(void);
#endif
