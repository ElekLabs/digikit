#include "frame.h"

/* Bank references are one-based. Zero means that no bank is published or playing. */
static inline __attribute__((always_inline)) int32_t
command(volatile PeerAudioWire *wire, PeerAudioState *state,
        int16_t pcm[PEER_AUDIO_BANKS][PEER_AUDIO_BANK_FRAMES])
{
    uint32_t bank = wire->bank;
    uint32_t generation = wire->generation;
    uint32_t offset = wire->offset;
    uint32_t count = wire->count;
    uint32_t op = wire->command;
    if (bank >= PEER_AUDIO_BANKS || generation == 0)
        return PEER_AUDIO_INVALID;
    PeerAudioBank *slot = &state->banks[bank];
    uint32_t ref = bank + 1;
    if (op == PEER_AUDIO_TRIGGER) {
        if (ref != state->published || slot->generation != generation ||
            count > PEER_AUDIO_GAIN_ONE)
            return PEER_AUDIO_INVALID;
        state->playing = ref;
        state->cursor = 0;
        state->gain = count;
        return PEER_AUDIO_OK;
    }
    if (ref == state->published || ref == state->playing)
        return PEER_AUDIO_IN_USE;
    if (op == PEER_AUDIO_WRITE) {
        if (count == 0 || count > PEER_AUDIO_CHUNK_FRAMES || offset > PEER_AUDIO_BANK_FRAMES ||
            count > PEER_AUDIO_BANK_FRAMES - offset)
            return PEER_AUDIO_INVALID;
        if (offset != 0 && (slot->frames != offset || slot->generation != generation))
            return PEER_AUDIO_INVALID;
        for (uint32_t i = 0; i < count; ++i)
            pcm[bank][offset + i] = wire->pcm[i];
        slot->generation = generation;
        slot->frames = offset + count;
        return PEER_AUDIO_OK;
    }
    if (op != PEER_AUDIO_PUBLISH || slot->generation != generation || !count ||
        slot->frames != count)
        return PEER_AUDIO_INVALID;
    state->published = ref;
    wire->published_generation = generation;
    wire->frames = count;
    return PEER_AUDIO_OK;
}

/* The image loader copies this section to peer DDR. This entry has no absolute code or data
 * references. Its arguments provide all the memory it needs. */
#ifdef __m68k__
__attribute__((section(".text.peer_audio")))
#endif
void peer_audio_frame(volatile PeerAudioWire *wire, PeerAudioState *state, int32_t *output,
                      int16_t pcm[PEER_AUDIO_BANKS][PEER_AUDIO_BANK_FRAMES])
{
    if (!state->ready) {
        wire->request = 0;
        wire->response = 0;
        wire->published = 0;
        wire->playing = 0;
        wire->published_generation = 0;
        wire->frames = 0;
        wire->version = PEER_AUDIO_V1;
        state->ready = 1;
    }
    uint32_t request = wire->request;
    uint32_t response = wire->response;
    if (request != response)
        wire->result = command(wire, state, pcm);

    uint32_t playing = state->playing;
    if (playing) {
        uint32_t cursor = state->cursor;
        uint32_t limit = state->banks[playing - 1].frames;
        uint32_t n = limit - cursor;
        if (n > PEER_AUDIO_BLOCK_FRAMES)
            n = PEER_AUDIO_BLOCK_FRAMES;
        int32_t scale = (int32_t)(state->gain << 8);
        for (uint32_t i = 0; i < n; ++i) {
            int32_t sample = pcm[playing - 1][cursor + i] * scale;
            int32_t mixed;
            if (__builtin_add_overflow(output[i], sample, &mixed))
                mixed = sample < 0 ? INT32_MIN : INT32_MAX;
            output[i] = mixed;
        }
        state->cursor = cursor + n;
        if (state->cursor == limit)
            state->playing = 0;
    }
    if (playing != state->playing || request != response)
        wire->playing = state->playing;
    if (request != response) {
        wire->published = state->published;
        wire->response = request;
    }
}
