/* The checks and copy of the dk_ui_text_input desc (text_input.h). Pure logic. */
#include "text_input.h"
#include <dk/core/status.h>

#ifndef DK_OK
#define DK_OK 0 /* as <dk/core/module.h> */
#endif

int32_t ui_text_input_check(const dk_ui_text_input_desc *d, uint32_t screen_max)
{
    if (!d || !d->done || !d->title)
        return DK_E_CONTRACT;
    if (d->max_len == 0 || d->max_len > DK_UI_TEXT_INPUT_MAX || d->max_len > screen_max)
        return DK_E_CONTRACT;
    return DK_OK;
}

void ui_text_input_initial(char out[DK_UI_TEXT_INPUT_MAX + 1], const dk_ui_text_input_desc *d)
{
    uint32_t i = 0;
    if (d->initial)
        while (d->initial[i] && i < d->max_len) {
            out[i] = d->initial[i];
            i++;
        }
    out[i] = 0;
}
