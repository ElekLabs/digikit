/* Host test of the dk_ui_text_input desc checks of core/ui (src/text_input.h, the same source ui.c links). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/text_input.h"

static void done(void *arg, const char *text)
{
    (void)arg;
    (void)text;
}

/* what: 0 a good desc, 1 no desc, 2 no done, 3 no title. */
int32_t ti_check(int what, uint32_t max_len, uint32_t screen_max)
{
    dk_ui_text_input_desc d = {"NAME", "KICK", max_len, done, 0};
    if (what == 1)
        return ui_text_input_check(0, screen_max);
    if (what == 2)
        d.done = 0;
    if (what == 3)
        d.title = 0;
    return ui_text_input_check(&d, screen_max);
}

/* The start text for `initial` (NULL: pass 0 for has_initial) cut to max_len; written to out (16 bytes). */
void ti_initial(const char *initial, int has_initial, uint32_t max_len, char *out)
{
    dk_ui_text_input_desc d = {"NAME", has_initial ? initial : 0, max_len, done, 0};
    char buf[DK_UI_TEXT_INPUT_MAX + 1];
    ui_text_input_initial(buf, &d);
    memcpy(out, buf, sizeof buf);
}
