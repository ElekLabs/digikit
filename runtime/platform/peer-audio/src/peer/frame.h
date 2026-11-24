#ifndef DK_PEER_AUDIO_FRAME_H
#define DK_PEER_AUDIO_FRAME_H

#include "../wire.h"

typedef struct PeerAudioBank {
    uint32_t generation;
    uint32_t frames;
} PeerAudioBank;

typedef struct PeerAudioState {
    PeerAudioBank banks[PEER_AUDIO_BANKS];
    uint32_t published;
    uint32_t playing;
    uint32_t cursor;
    uint32_t gain;
    uint32_t selected_track; /* one-based; zero leaves the stock tracks alone */
    uint32_t ready;
} PeerAudioState;

_Static_assert(sizeof(PeerAudioState) <= 1024, "peer audio state fits its DDR reservation");

void peer_audio_frame(volatile PeerAudioWire *wire, PeerAudioState *state, int32_t *output,
                      int16_t pcm[PEER_AUDIO_BANKS][PEER_AUDIO_BANK_FRAMES], uint32_t notes);

#endif
