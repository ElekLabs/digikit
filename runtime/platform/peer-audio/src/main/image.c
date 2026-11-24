#include "../wire.h"

extern const uint8_t peer_audio_entry[], peer_audio_render[], peer_audio_end[];
extern volatile PeerAudioWire *const peer_audio_mailbox;
extern const struct {
    uint32_t base, entry, stack, stack_operand, render, render_operand, receiver, state;
} peer_audio_layout;

/* The uncached stream starts at an unaligned address. */
static uint32_t load_word(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

static void store_word(uint8_t *p, uint32_t value)
{
    p[0] = value >> 24;
    p[1] = value >> 16;
    p[2] = value >> 8;
    p[3] = value;
}

uint32_t dk_peer_image_extend(uint8_t *image, uint32_t length, uint32_t capacity)
{
    uint32_t bytes = (uintptr_t)peer_audio_end - (uintptr_t)peer_audio_entry;
    uint32_t resident = (uintptr_t)peer_audio_end - (uintptr_t)peer_audio_render;
    uint32_t stack = peer_audio_layout.stack_operand - peer_audio_layout.base;
    uint32_t render = peer_audio_layout.render_operand - peer_audio_layout.base;
    if (length > capacity || bytes > capacity - length || resident == 0 || (resident & 3) ||
        resident > peer_audio_layout.state - peer_audio_layout.receiver || length < stack + 4 ||
        length < render + 4 || load_word(image) != peer_audio_layout.entry ||
        load_word(image + stack) != peer_audio_layout.stack ||
        load_word(image + render) != peer_audio_layout.render)
        return length;

    for (uint32_t i = 0; i < bytes; ++i)
        image[length + i] = peer_audio_entry[i];
    store_word(image + stack, peer_audio_layout.receiver);
    store_word(image + render, peer_audio_layout.receiver);
    store_word(image, peer_audio_layout.base + length);
    peer_audio_mailbox->version = 0;
    return length + bytes;
}
