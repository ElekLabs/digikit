/* ui_text.h: pixel widths and pixel padding of list text, shared by core/ui and the menu of the system package.
   The list font is not monospace: a letter, a digit, '-' or '_' takes 4 pixels, a space 3, '.' 2 and '~' 5 (measured on the DT2 1.16 list
   font). Character 0x01 draws nothing and takes 4 pixels, so it pads a column the width of one letter. */
#ifndef DK_UI_TEXT_H
#define DK_UI_TEXT_H
#include <stdint.h>

#define DK_UI_PAD '\x01'

/* The width of `s` in pixels. */
static inline int dk_ui_px_of(const char *s)
{
    int w = 0;
    for (; *s; s++)
        w += *s == ' ' ? 3 : *s == '.' ? 2 : *s == '~' ? 5 : 4;
    return w;
}

/* Appends spaces (3 pixels) and PAD (4 pixels) to the NUL terminated text `buf` of `len` characters until it is `px` pixels wide or the next step
   would pass it; never past `size - 1` characters. Returns the new length. */
static inline int dk_ui_pad_px(char *buf, int len, int size, int px)
{
    int rest = px - dk_ui_px_of(buf);
    while (rest >= 3 && rest % 4 && len < size - 1) {
        buf[len++] = ' ';
        rest -= 3;
    }
    for (; rest >= 4 && len < size - 1; rest -= 4)
        buf[len++] = DK_UI_PAD;
    buf[len] = 0;
    return len;
}

/* As dk_ui_pad_px, with spaces only, to the nearest 3 pixels: for text in a font that draws PAD (the choice popup shows a box for 0x01; the list does not). */
static inline int dk_ui_pad_spaces(char *buf, int len, int size, int px)
{
    for (int rest = px - dk_ui_px_of(buf); rest >= 2 && len < size - 1; rest -= 3)
        buf[len++] = ' ';
    buf[len] = 0;
    return len;
}

#endif
