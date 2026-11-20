/* dk_ui_text_input (<dk/platform/ui.h>): the checks and the copy of its desc, with no firmware, so the host can test them
   (modules/core/adapters/ui/host/tests/text_input.rs). The screen itself is the firmware's NameView, built in ui.c. */
#ifndef DK_UI_TEXT_INPUT_H
#define DK_UI_TEXT_INPUT_H
#include <stdint.h>
#include <dk/platform/ui.h>

/* DK_OK, or DK_E_CONTRACT: no desc, no `done`, no `title`, or `max_len` 0, above DK_UI_TEXT_INPUT_MAX or above `screen_max` (how many characters the
   firmware's screen takes). */
int32_t ui_text_input_check(const dk_ui_text_input_desc *d, uint32_t screen_max);

/* The start text of a checked desc: `initial` cut to max_len characters, empty for NULL. */
void ui_text_input_initial(char out[DK_UI_TEXT_INPUT_MAX + 1], const dk_ui_text_input_desc *d);

#endif
