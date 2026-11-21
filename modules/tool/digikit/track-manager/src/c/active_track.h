/* active_track: the track the stock firmware has selected. The one place that reads it. */
#ifndef ACTIVE_TRACK_H
#define ACTIVE_TRACK_H
#include <stdint.h>

/* The selected track from 0, or -1 when there is no project. */
int32_t active_track(void);

/* The selected track, or track 0 when there is no project. */
uint32_t active_track_or_first(void);

#endif
