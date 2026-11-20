/* active_track: the track the stock firmware has selected. The one place that reads it. */
#ifndef ACTIVE_TRACK_H
#define ACTIVE_TRACK_H
#include <stdint.h>

/* The selected track from 0, or 0 when the project does not give one yet. */
uint32_t active_track(void);

#endif
