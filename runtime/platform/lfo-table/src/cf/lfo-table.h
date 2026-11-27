/* core/lfo-table: the interface of the added LFO waves (version 1). The stub of each wave is lfo-wave.inc; the two calls below are lfo-table.c.
 * The words of `lfo_table_info` (the image's info table, name "lfo_table_info") are the first lines of the enum. */
#ifndef LFO_TABLE_H
#define LFO_TABLE_H

#include <stdint.h>

/* Samples of a shape: the LFO reads them at the top 5 bits of its phase and interpolates with the next 15 bits. */
#define LFO_SAMPLES 32

/* One cell of an added wave, as the stub reads it (lfo-wave.inc): the sample as Q31 and the step to the next sample. */
typedef struct {
    int32_t v; /* samples[i] << 16 */
    int32_t d; /* samples[(i + 1) & 31] - samples[i] */
} lfo_cell;

#define LFO_INFO_MAGIC 0x4c464f54u /* 'LFOT' */
#define LFO_INFO_VERSION 1u

enum lfo_info_word {
    LFO_INFO_W_MAGIC,
    LFO_INFO_W_VERSION,
    LFO_INFO_W_FIRST,   /* the first added wave number (7) */
    LFO_INFO_W_WAVES,   /* how many (N, [limits] lfo_waves) */
    LFO_INFO_W_SAMPLES, /* samples of a shape (32) */
    LFO_INFO_W_SET,     /* int32_t lfo_table_set(uint32_t wave, const int16_t samples[32]) */
    LFO_INFO_W_CLEAR,   /* void lfo_table_clear(uint32_t wave) */
    LFO_INFO_WORDS
};

#define LFO_SINE 1 /* the slot of the stock sine in the function table: what an empty wave plays */

/* Gives wave `wave` the 32 samples: its cells are written, then its function entry points at its stub, all under the interrupt mask. UI task only.
 * Returns 0, or -1 (nothing changed) when `wave` is outside first .. first + N - 1. */
int32_t lfo_table_set(uint32_t wave, const int16_t *samples);

/* Empties wave `wave`: its function entry becomes the stock sine again, under the interrupt mask. A wave outside the range is ignored. UI task only. */
void lfo_table_clear(uint32_t wave);

#endif
