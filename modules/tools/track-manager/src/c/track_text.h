/* track_text: the numbers shown on screen (tracks count from 1 there, from 0 in the code). Pure logic. */
#ifndef TRACK_TEXT_H
#define TRACK_TEXT_H
#include <stdint.h>

/* The stock font: glyphs are 6 pixels high. Layout rows are multiples of it plus a gap; widths come from `dk_canvas.text_width`, never from a character count. */
#define TM_GLYPH_H 6

/* Two digits of `track + 1` and a terminator (tracks 100 and up show the last two digits). */
static inline void track_text_number(char out[3], uint32_t track)
{
    uint32_t n = track + 1, tens = 0;
    while (n >= 100)
        n -= 100;
    while (n >= 10) {
        n -= 10;
        tens++;
    }
    out[0] = (char)('0' + tens);
    out[1] = (char)('0' + n);
    out[2] = 0;
}

#endif
