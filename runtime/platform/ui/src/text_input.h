/* dk_ui_text_input (<dk/platform/ui.h>): the owner object of the one open text input and the checks of its desc, with no firmware, so the host can test them
   (modules/core/adapters/ui/host/tests/text_input.rs). The screen itself is the firmware's NameView, built in ui.c behind `ui_text_fw`.
   One input at a time. Its life: `ui_text_open` takes the input (busy), the firmware screen calls `ui_text_done` on YES (before it closes) and `ui_text_gone` when it left the
   stack in any way (NO has no done). When the module that opened it unloads, `ui_text_drop_owner` keeps the screen but never calls the module's `done`: the code is gone. */
#ifndef DK_UI_TEXT_INPUT_H
#define DK_UI_TEXT_INPUT_H
#include <stdint.h>
#include <dk/platform/ui.h>
#include <dk/gen/log_kinds.h>
#include "key_defer.h"

/* DK_OK, or DK_E_CONTRACT: no desc, no `done`, no `title`, or `max_len` 0, above DK_UI_TEXT_INPUT_MAX or above `screen_max` (how many characters the
   firmware's screen takes). */
int32_t ui_text_input_check(const dk_ui_text_input_desc *d, uint32_t screen_max);

/* The start text of a checked desc: `initial` cut to max_len characters, empty for NULL. */
void ui_text_input_initial(char out[DK_UI_TEXT_INPUT_MAX + 1], const dk_ui_text_input_desc *d);

/* What the owner object asks of the firmware. */
typedef struct ui_text_fw {
    void *fw;
    /* Build and push the naming screen for `d` with the start text `initial`; the screen calls ui_text_done on YES and ui_text_gone(serial) when it left the stack.
       0 when shown, else a negative code (the screen is not shown and calls nothing). */
    int32_t (*push)(void *fw, const dk_ui_text_input_desc *d, const char *initial, uint32_t serial);
    /* A log event (`DK_LOG_TEXT_INPUT_OPENED` or `DK_LOG_TEXT_INPUT_DONE` of <dk/gen/log_kinds.h>) with its flags (always `DK_LOG_F_KEEP`), the owner handle and a word. */
    void (*emit)(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a);
} ui_text_fw;

typedef struct ui_text_input {
    const ui_text_fw *fw;
    ui_defer *
        defer; /* a press callback that opens the input: the screen is pushed after the key release (key_defer.h) */
    void (*done)(void *arg, const char *text); /* 0 once the owner is gone */
    void *arg;
    uintptr_t owner;
    uint32_t open;   /* serial of the open input, 0: none */
    uint8_t pending; /* the push waits for the key release */
    /* the desc as copied at the call (it may be on the caller's stack): kept while the push is pending */
    char title[DK_UI_ROW];
    char initial[DK_UI_TEXT_INPUT_MAX + 1];
    uint32_t max_len;
    uint32_t serial;
} ui_text_input;

/* Open the input for `owner`: the checks, then DK_E_BUSY while another input is on the screen, then the firmware's push. DK_OK or a negative code. */
int32_t ui_text_open(ui_text_input *t, uintptr_t owner, const dk_ui_text_input_desc *d,
                     uint32_t screen_max);

/* Firmware to owner object: YES, with the text. The input is free again before `done` runs, so `done` may open the next one. */
void ui_text_done(ui_text_input *t, const char *text);

/* Firmware to owner object: the screen `serial` left the stack. A screen of an earlier input does not end the open one. */
void ui_text_gone(ui_text_input *t, uint32_t serial);

/* The module `owner` unloads: if its input is open, `done` is never called (the screen stays until the user leaves it). */
void ui_text_drop_owner(ui_text_input *t, uintptr_t owner);

#endif
