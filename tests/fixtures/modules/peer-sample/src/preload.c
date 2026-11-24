#include "dkrt.h"

#define HEADER_BYTES 64u
#define TRAILER_BYTES 16u
#define SECTOR_BYTES 512u
#define STATE_BYTES 8u
#define MAX_PCM_BYTES 4096u
#define READ_BYTES                                                                                 \
    ((HEADER_BYTES + MAX_PCM_BYTES + TRAILER_BYTES + SECTOR_BYTES - 1) & ~(SECTOR_BYTES - 1))

extern const uint8_t peer_sample_blob[], peer_sample_entry[], peer_sample_frame[];
extern const uint8_t peer_sample_state[];
extern const struct {
    uint32_t base, entry, stack, stack_operand, render, render_operand, reservation;
} peer_sample_layout;

static uint8_t sample_file[READ_BYTES] __attribute__((aligned(16)));
static uint32_t sample_pcm_bytes;

static uint32_t read_be32(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

static void write_be32(uint8_t *p, uint32_t value)
{
    p[0] = value >> 24;
    p[1] = value >> 16;
    p[2] = value >> 8;
    p[3] = value;
}

static uint32_t mono_pcm_bytes(void)
{
    static const uint8_t path[] = "samples/peer-proof";
    DkrtStoreFile file;
    int32_t lease = dkrt_store_lease(path, &file);
    if (lease <= 0)
        return 0;
    int read = 0;
    if (file.size >= HEADER_BYTES && file.size <= HEADER_BYTES + MAX_PCM_BYTES + TRAILER_BYTES) {
        uint32_t sectors = (file.size + SECTOR_BYTES - 1) / SECTOR_BYTES;
        read = dkrt_store_read(&file, 0, sectors, sample_file) == 0;
    }
    dkrt_store_unlease(lease);
    if (!read)
        return 0;
    uint32_t pcm = read_be32(sample_file + 4);
    if (sample_file[0] != 0 || sample_file[1] != 0 || sample_file[0x14] != 0x7f ||
        read_be32(sample_file + 8) != 48000 || (pcm & 1) || pcm == 0 || pcm > MAX_PCM_BYTES ||
        (file.size != HEADER_BYTES + pcm && file.size != HEADER_BYTES + pcm + TRAILER_BYTES))
        return 0;
    return pcm;
}

/* Run on the card-owning stock boot task before it starts the peer thread. */
void dk_peer_sample_preload(void)
{
    sample_pcm_bytes = mono_pcm_bytes();
}

/* core/peer-image has checked the stock SHA before calling. A missing or invalid file leaves the stream stock. */
uint32_t dk_peer_image_extend(uint8_t *image, uint32_t length, uint32_t capacity)
{
    uint32_t pcm = sample_pcm_bytes;
    if (!pcm)
        return length;

    uint32_t blob = (uintptr_t)peer_sample_state - (uintptr_t)peer_sample_blob;
    uint32_t frames = pcm / 2;
    uint32_t padded = (pcm + 3u) & ~3u;
    uint32_t resident =
        (uintptr_t)peer_sample_state - (uintptr_t)peer_sample_frame + STATE_BYTES + padded;
    uint32_t extra = blob + STATE_BYTES + padded;
    uint32_t stack_at = peer_sample_layout.stack_operand - peer_sample_layout.base;
    uint32_t render_at = peer_sample_layout.render_operand - peer_sample_layout.base;
    if (length > capacity || extra > capacity - length ||
        resident > peer_sample_layout.stack - peer_sample_layout.reservation ||
        read_be32(image) != peer_sample_layout.entry ||
        read_be32(image + stack_at) != peer_sample_layout.stack ||
        read_be32(image + render_at) != peer_sample_layout.render)
        return length;

    uint8_t *append = image + length;
    for (uint32_t i = 0; i < blob; ++i)
        append[i] = peer_sample_blob[i];
    uint8_t *state = append + blob;
    write_be32(state, frames);
    write_be32(state + 4, frames);
    for (uint32_t i = 0; i < pcm; ++i)
        state[STATE_BYTES + i] = sample_file[HEADER_BYTES + i];
    for (uint32_t i = pcm; i < padded; ++i)
        state[STATE_BYTES + i] = 0;

    write_be32(image + stack_at, peer_sample_layout.reservation);
    write_be32(image + render_at, peer_sample_layout.reservation);
    uint32_t entry = (uintptr_t)peer_sample_entry - (uintptr_t)peer_sample_blob;
    write_be32(image, peer_sample_layout.base + length + entry);
    return length + extra;
}
