#ifndef BOOT_SCENE_ASSET_H
#define BOOT_SCENE_ASSET_H

#include <stdint.h>

/* Private RAM decoder. Source bytes must remain immutable for the state's lifetime. */
typedef struct {
    const uint8_t *data;
    uint32_t size, cursor, records, total_ticks;
    uint32_t record, tick, hold, decoded_records;
    int valid;
    uint8_t pixels[1024] __attribute__((aligned(4)));
} Dkbs;

/* Full validation before success (1); failure (0) leaves next() inert. */
int dkbs_open(Dkbs *state, const uint8_t *data, uint32_t size);
/* One expanded tick (1), exact completion (0), invalid/inert (-1). */
int dkbs_next(Dkbs *state);

#endif
