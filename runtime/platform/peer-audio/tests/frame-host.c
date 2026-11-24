#include "../src/peer/frame.h"
#include <assert.h>
#include <string.h>

static PeerAudioWire wire;
static PeerAudioState state;
static int16_t pcm[PEER_AUDIO_BANKS][PEER_AUDIO_BANK_FRAMES];
static int32_t audio[PEER_AUDIO_BLOCK_FRAMES];

static void frame(void)
{
    memset(audio, 0, sizeof(audio));
    peer_audio_frame(&wire, &state, audio, pcm);
}

static int32_t send(unsigned command, unsigned bank, unsigned generation, unsigned offset,
                    unsigned count)
{
    wire.command = command;
    wire.bank = bank;
    wire.generation = generation;
    wire.offset = offset;
    wire.count = count;
    ++wire.request;
    frame();
    assert(wire.response == wire.request);
    return wire.result;
}

static void publish(unsigned bank, unsigned generation, unsigned count, int16_t value)
{
    for (unsigned i = 0; i < count; ++i)
        wire.pcm[i] = value;
    assert(send(PEER_AUDIO_WRITE, bank, generation, 0, count) == PEER_AUDIO_OK);
    assert(send(PEER_AUDIO_PUBLISH, bank, generation, 0, count) == PEER_AUDIO_OK);
    assert(wire.published == bank + 1 && wire.published_generation == generation);
}

int main(void)
{
    frame();
    assert(wire.version == PEER_AUDIO_V1);
    assert(send(PEER_AUDIO_WRITE, 0, 1, 0, PEER_AUDIO_CHUNK_FRAMES + 1) == PEER_AUDIO_INVALID);
    assert(send(PEER_AUDIO_WRITE, 0, 1, PEER_AUDIO_BANK_FRAMES - 1, 2) == PEER_AUDIO_INVALID);
    assert(send(PEER_AUDIO_WRITE, 0, 1, 1, 5) == PEER_AUDIO_INVALID);
    assert(send(PEER_AUDIO_PUBLISH, 0, 1, 0, 5) == PEER_AUDIO_INVALID);

    publish(0, 1, 160, 7);
    assert(send(PEER_AUDIO_TRIGGER, 0, 1, 0, PEER_AUDIO_GAIN_ONE) == PEER_AUDIO_OK);
    assert(audio[0] == 7 * 65536 && audio[31] == audio[0] && wire.playing == 1);
    publish(1, 2, 33, -9);
    assert(audio[0] == 7 * 65536 && wire.playing == 1);
    assert(send(PEER_AUDIO_WRITE, 0, 3, 0, 1) == PEER_AUDIO_IN_USE);
    assert(wire.playing == 1);
    frame();
    assert(wire.playing == 0 && audio[31] == 7 * 65536);
    assert(send(PEER_AUDIO_WRITE, 0, 3, 0, 1) == PEER_AUDIO_OK);
    assert(send(PEER_AUDIO_TRIGGER, 1, 2, 0, 128) == PEER_AUDIO_OK);
    assert(audio[0] == -9 * 32768 && audio[31] == audio[0] && wire.playing == 2);
    frame();
    assert(audio[0] == -9 * 32768 && audio[1] == 0 && wire.playing == 0);
    return 0;
}
