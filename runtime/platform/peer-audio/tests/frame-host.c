#include "../src/peer/frame.h"
#include <assert.h>
#include <string.h>

static PeerAudioWire wire;
static PeerAudioState state;
static int16_t pcm[PEER_AUDIO_BANKS][PEER_AUDIO_BANK_FRAMES];
static int32_t audio[PEER_AUDIO_TRACKS * PEER_AUDIO_BLOCK_FRAMES];

static void frame(void)
{
    memset(audio, 0, sizeof(audio));
    peer_audio_frame(&wire, &state, audio, pcm, 0);
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

static void test_publication(void)
{
    frame();
    assert(wire.version == PEER_AUDIO_V2);
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
}

static void note(uint32_t notes)
{
    for (unsigned i = 0; i < sizeof(audio) / sizeof(audio[0]); ++i)
        audio[i] = 11;
    peer_audio_frame(&wire, &state, audio, pcm, notes);
    assert(audio[0] == 11 && audio[2 * PEER_AUDIO_BLOCK_FRAMES] == 11);
}

static void test_selected_track(void)
{
    static const unsigned track = 2;
    int32_t *lane = audio + PEER_AUDIO_BLOCK_FRAMES;
    memset(&wire, 0, sizeof(wire));
    memset(&state, 0, sizeof(state));
    frame();
    assert(send(PEER_AUDIO_SELECT_TRACK, 0, 0, 0, PEER_AUDIO_TRACKS + 1) == PEER_AUDIO_INVALID);
    assert(send(PEER_AUDIO_SELECT_TRACK, 0, 0, 0, track) == PEER_AUDIO_OK);
    assert(wire.selected_track == track);
    note(1u << (track - 1));
    assert(wire.note_starts == 0 && lane[0] == 0);

    for (unsigned i = 0; i < 80; ++i)
        wire.pcm[i] = (int16_t)(1 + i);
    assert(send(PEER_AUDIO_WRITE, 0, 1, 0, 80) == PEER_AUDIO_OK);
    assert(send(PEER_AUDIO_PUBLISH, 0, 1, 0, 80) == PEER_AUDIO_OK);
    assert(send(PEER_AUDIO_TRIGGER, 0, 1, 0, PEER_AUDIO_GAIN_ONE) == PEER_AUDIO_IN_USE);
    note(1u);
    assert(wire.note_starts == 0 && lane[0] == 0);
    note(1u << (track - 1));
    assert(wire.note_starts == 1 && wire.playing == 1 && lane[0] == 65536);
    note(0);
    assert(wire.note_starts == 1 && lane[0] == 33 * 65536);
    note(1u << (track - 1));
    assert(wire.note_starts == 2 && lane[0] == 65536);
    assert(send(PEER_AUDIO_SELECT_TRACK, 0, 0, 0, 0) == PEER_AUDIO_OK);
    note(1u << (track - 1));
    assert(wire.note_starts == 2 && wire.playing == 0 && lane[0] == 11);
}

int main(void)
{
    test_publication();
    test_selected_track();
    return 0;
}
