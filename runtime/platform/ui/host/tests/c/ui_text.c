/* The pixel helpers of core/ui and the menu (<dk/platform/ui_text.h>), for tests/ui_text.rs. */
#include <string.h>
#include <dk/platform/ui_text.h>

int t_px(const char *s)
{
    return dk_ui_px_of(s);
}

/* Pads the text in `buf` (`size` bytes) to `px` pixels; the new length. */
int t_pad(char *buf, int size, int px)
{
    return dk_ui_pad_px(buf, (int)strlen(buf), size, px);
}

int t_pad_spaces(char *buf, int size, int px)
{
    return dk_ui_pad_spaces(buf, (int)strlen(buf), size, px);
}
