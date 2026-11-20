/* active_track: see active_track.h. It reads the selected track through the runtime import `dk_selected_track` (UI task). */
#include "active_track.h"

#include <dk/core/module.h>

uint32_t active_track(void)
{
    int32_t track = dk_selected_track();
    return track < 0 ? 0 : (uint32_t)track;
}
