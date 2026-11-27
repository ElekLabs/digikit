/* active_track: see active_track.h. It reads the selected track through the runtime import `dk_selected_track` (UI task). */
#include "active_track.h"

#include <dk/core/module.h>

int32_t active_track(void)
{
    return dk_selected_track();
}

uint32_t active_track_or_first(void)
{
    int32_t track = active_track();
    return track < 0 ? 0 : (uint32_t)track;
}
