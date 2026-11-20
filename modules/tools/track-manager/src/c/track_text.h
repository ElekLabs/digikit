/* track_text: the numbers shown on screen (tracks count from 1 there, from 0 in the code). Pure logic. */
#ifndef TRACK_TEXT_H
#define TRACK_TEXT_H
#include <stdint.h>

/* Two digits of `track + 1` and a terminator (tracks 100 and up show the last two digits). */
static inline void track_text_number(char out[3], uint32_t track)
{
    uint32_t n = (track + 1) % 100;
    out[0] = (char)('0' + n / 10);
    out[1] = (char)('0' + n % 10);
    out[2] = 0;
}

#endif
