#include "asset.h"

static uint32_t be16(const uint8_t *p)
{
    return ((uint32_t)p[0] << 8) | p[1];
}

static uint32_t be32(const uint8_t *p)
{
    return (be16(p) << 16) | be16(p + 2);
}

static uint32_t crc32(const uint8_t *p, uint32_t size)
{
    uint32_t crc = 0xffffffffu;
    for (uint32_t i = 0; i < size; i++) {
        crc ^= p[i];
        for (uint32_t bit = 0; bit < 8; bit++)
            crc = (crc >> 1) ^ ((crc & 1u) ? 0xedb88320u : 0u);
    }
    return crc ^ 0xffffffffu;
}

static void rewind_asset(Dkbs *s)
{
    s->cursor = 16;
    s->record = s->tick = s->hold = s->decoded_records = 0;
    for (uint32_t i = 0; i < 1024; i++)
        s->pixels[i] = 0;
}

static int invalid(Dkbs *s)
{
    s->valid = 0;
    return -1;
}

static int record(Dkbs *s)
{
    if (s->cursor > s->size || s->size - s->cursor < 8 || s->record >= s->records)
        return invalid(s);
    const uint8_t *header = s->data + s->cursor;
    uint32_t hold = be16(header), size = be16(header + 2), codec = header[4];
    s->cursor += 8;
    if (!hold || hold > 300 || hold > s->total_ticks - s->tick || !size ||
        size > s->size - s->cursor || codec > 1 || header[5] || header[6] || header[7])
        return invalid(s);
    const uint8_t *payload = s->data + s->cursor;
    if (!codec) {
        if (size != 1024)
            return invalid(s);
        for (uint32_t i = 0; i < 1024; i++)
            s->pixels[i] = payload[i];
    } else {
        uint32_t at = 0, out = 0;
        while (at < size) {
            uint32_t ctrl = payload[at++], count = (ctrl & 127u) + 1;
            uint32_t take = (ctrl & 128u) ? 1 : count;
            if (count > 1024 - out || take > size - at)
                return invalid(s);
            for (uint32_t i = 0; i < count; i++)
                s->pixels[out++] ^= payload[at + ((ctrl & 128u) ? 0 : i)];
            at += take;
        }
        if (out != 1024)
            return invalid(s);
    }
    s->cursor += size;
    s->hold = hold;
    s->record++;
    s->decoded_records++;
    return 1;
}

int dkbs_next(Dkbs *s)
{
    if (!s->valid)
        return -1;
    if (s->tick == s->total_ticks) {
        if (s->hold || s->record != s->records || s->cursor != s->size)
            return invalid(s);
        return 0;
    }
    if (s->tick > s->total_ticks || (!s->hold && record(s) != 1))
        return invalid(s);
    s->hold--;
    s->tick++;
    return 1;
}

int dkbs_open(Dkbs *s, const uint8_t *data, uint32_t size)
{
    s->valid = 0;
    s->data = 0;
    s->size = s->records = s->total_ticks = 0;
    rewind_asset(s);
    if (!data || size < 16 || size > 65536 || data[0] != 'D' || data[1] != 'K' ||
        data[2] != 'B' || data[3] != 'S' || be16(data + 4) != 1)
        return 0;
    uint32_t records = be16(data + 6), ticks = be32(data + 8);
    if (!records || records > 300 || !ticks || ticks > 300 || crc32(data + 16, size - 16) != be32(data + 12))
        return 0;
    s->data = data;
    s->size = size;
    s->records = records;
    s->total_ticks = ticks;
    s->valid = 1;
    for (uint32_t i = 0; i < ticks; i++) {
        if (dkbs_next(s) != 1)
            return 0;
    }
    if (dkbs_next(s) != 0)
        return 0;
    rewind_asset(s);
    return 1;
}
