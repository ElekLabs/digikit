/* Opt-in observation of the top stock UI view. The runtime owns the port
 * readback, and this module owns the enabled state, baseline and change log. */
#include "view-observer.h"

#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/gen/log_kinds.h>
#include <dk/platform/debug.h>

#define VIEW_GET 4u
#define VIEW_SET 5u
#define VIEW_PROTOCOL 1u

/* The MIDI task changes the requested generation. The UI task alone publishes
 * snapshots. Generation zero is off. A set never publishes an old baseline. */
static volatile uint32_t active;
static uint32_t next;
static volatile struct {
    uint32_t revision, generation, object, class_id;
} snapshot;

int32_t debug_view_observer_config(const uint8_t *body, uint32_t len, uint8_t *out)
{
    uint32_t generation, revision, object, class_id;
    uint8_t ready, supported;
    if (len != (body[0] == VIEW_GET ? 1u : 2u) || (len == 2u && body[1] > 1u))
        return DK_E_MSG;
    supported = dk_debug_top_view(0) == 0;
    if (body[0] == VIEW_SET) {
        if (body[1] && !supported)
            return DK_E_NOT_FOUND;
        if (body[1] == 0u) {
            active = 0u;
        } else {
            if (active != 0u)
                return DK_E_BUSY;
            if (++next == 0u)
                ++next;
            active = next;
        }
    }
    generation = active;
    revision = snapshot.revision;
    object = snapshot.object;
    class_id = snapshot.class_id;
    ready = generation && snapshot.generation == generation && !(revision & 1u) &&
            revision == snapshot.revision;
    out[0] = VIEW_PROTOCOL;
    out[1] = supported;
    out[2] = generation != 0u;
    out[3] = ready;
    dk_bytes_put32(out + 4u, generation);
    dk_bytes_put32(out + 8u, ready ? object : 0u);
    dk_bytes_put32(out + 12u, ready ? class_id : 0u);
    return 16;
}

void debug_view_observer_tick(void)
{
    uint32_t generation = active;
    uint32_t current[2];
    uint32_t old_object, old_class, had_baseline;
    if (generation == 0u || dk_debug_top_view(current) != 0 || generation != active)
        return;

    old_object = snapshot.object;
    old_class = snapshot.class_id;
    had_baseline = snapshot.generation == generation;
    if (had_baseline && current[0] == old_object && current[1] == old_class)
        return;

    snapshot.revision++;
    snapshot.object = current[0];
    snapshot.class_id = current[1];
    snapshot.generation = generation;
    snapshot.revision++;

    if (had_baseline && generation == active) {
        uint32_t args[5] = {old_object, current[0], old_class, current[1], generation};
        dk_log_event(DK_LOG_VIEW_CHANGED, DK_LOG_DEBUG << 1, args);
    }
}
