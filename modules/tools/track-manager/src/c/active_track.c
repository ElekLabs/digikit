/* active_track: see active_track.h. It reads the selected track through the debug import `dk_debug_selected_params`, which is the only call that
 * gives it now; a non-debug import replaces this one function later. */
#include "active_track.h"

#include <dk/core/module.h>

uint32_t active_track(void)
{
    int16_t unused[1];
    int32_t track = dk_debug_selected_params(unused, 1);
    return track < 0 ? 0 : (uint32_t)track;
}
