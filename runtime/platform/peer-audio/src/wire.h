#ifndef DK_PEER_AUDIO_WIRE_H
#define DK_PEER_AUDIO_WIRE_H

#include <stdint.h>

enum {
    PEER_AUDIO_V1 = 0x44504101,
    PEER_AUDIO_BANKS = 2,
    PEER_AUDIO_BANK_FRAMES = 32768,
    PEER_AUDIO_CHUNK_FRAMES = 224,
    PEER_AUDIO_BLOCK_FRAMES = 32,
    PEER_AUDIO_GAIN_ONE = 256,
};

enum PeerAudioCommand {
    PEER_AUDIO_WRITE = 1,
    PEER_AUDIO_PUBLISH,
    PEER_AUDIO_TRIGGER,
};

enum PeerAudioReply {
    PEER_AUDIO_OK = 0,
    PEER_AUDIO_INVALID = -1,
    PEER_AUDIO_IN_USE = -2,
};

/* MAIN writes request last. The peer writes response last. MAIN leaves the command and PCM alone
 * until both words match. All words have the native big-endian representation on both CPUs. */
typedef struct PeerAudioWire {
    uint32_t version;
    uint32_t request;
    uint32_t response;
    uint32_t command;
    uint32_t bank;
    uint32_t generation;
    uint32_t offset;
    uint32_t count;
    int32_t result;
    uint32_t published;
    uint32_t playing;
    uint32_t published_generation;
    uint32_t frames;
    int16_t pcm[PEER_AUDIO_CHUNK_FRAMES];
} PeerAudioWire;

_Static_assert(sizeof(PeerAudioWire) == 500, "peer audio mailbox fits in window 0x600..0x7ff");

#endif
