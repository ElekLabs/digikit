#include "parameter-observer.h"

#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/gen/log_kinds.h>
#include <dk/platform/debug.h>

#define PARAM_PROBE 6u
#define PARAM_START 7u
#define PARAM_STATE 8u
#define PARAM_STOP 9u

/* MIDI owns the request; the UI task owns value reads and publication.
 * One requested watch is enough for a parameter edit or a baseline read. */
static volatile struct {
    uint32_t generation;
    uint8_t scope, track, word;
} request;
static uint32_t next;
static volatile struct {
    uint32_t generation;
    int32_t value;
} snapshot;

int32_t debug_parameter_observer_config(const uint8_t *body, uint32_t len, uint8_t *out)
{
    uint32_t generation, scope, track, word, ready;
    int32_t value;
    uint8_t supported = 0u;
    if (body[0] == PARAM_PROBE || body[0] == PARAM_START) {
        if (len != 4u)
            return DK_E_MSG;
        scope = body[1];
        track = body[2];
        word = body[3];
        supported = dk_debug_read_parameter(scope, track, word, 0) == 0;
        if (body[0] == PARAM_START) {
            if (!supported)
                return DK_E_NOT_FOUND;
            if (request.generation != 0u)
                return DK_E_BUSY;
            if (++next == 0u)
                ++next;
            request.scope = (uint8_t)scope;
            request.track = (uint8_t)track;
            request.word = (uint8_t)word;
            request.generation = next;
        }
    } else if (body[0] == PARAM_STATE) {
        if (len != 1u)
            return DK_E_MSG;
    } else if (body[0] == PARAM_STOP) {
        if (len != 5u)
            return DK_E_MSG;
        if (request.generation == dk_bytes_get32(body + 1u))
            request.generation = 0u;
    } else {
        return DK_E_PROTO;
    }
    generation = request.generation;
    /* A probe reports support for the requested key, not the active key. */
    if (body[0] == PARAM_PROBE) {
        out[2] = body[1];
        out[3] = body[2];
        word = body[3];
        ready = 0;
        value = 0;
    } else {
        out[2] = generation ? request.scope : 0u;
        out[3] = generation ? request.track : 0u;
        word = generation ? request.word : 0u;
        supported = generation != 0u;
        ready = generation && snapshot.generation == generation;
        value = ready ? snapshot.value : 0;
    }
    out[0] = 1u; /* protocol version */
    out[1] = supported | (generation ? 2u : 0u) | (ready ? 4u : 0u);
    dk_bytes_put32(out + 4u, generation);
    dk_bytes_put32(out + 8u, (uint32_t)value);
    dk_bytes_put32(out + 12u, word);
    return 16;
}

void debug_parameter_observer_tick(void)
{
    uint32_t generation = request.generation;
    uint32_t scope, track, word, had_snapshot;
    int32_t value, old;
    if (!generation)
        return;
    scope = request.scope;
    track = request.track;
    word = request.word;
    if (dk_debug_read_parameter(scope, track, word, &value) != 0 ||
        generation != request.generation)
        return;
    had_snapshot = snapshot.generation == generation;
    old = snapshot.value;
    if (had_snapshot && old == value)
        return;
    snapshot.value = value;
    snapshot.generation = generation;
    if (had_snapshot && generation == request.generation) {
        uint32_t args[5] = {scope, (track << 8) | word, (uint32_t)old, (uint32_t)value, generation};
        dk_log_event(DK_LOG_VALUE_CHANGED, DK_LOG_DEBUG << 1, args);
    }
}
