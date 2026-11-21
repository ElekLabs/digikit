/* track_names: see track_names.h. */
#include "track_names.h"

#include <dk/core/text.h> /* dk_text_len: the module has no C library */

/* byte loops, no <string.h> (the packer links the runtime's helpers if the compiler turns them into calls) */
static void zero(void *p, uint32_t n)
{
    uint8_t *b = p;
    while (n--)
        *b++ = 0;
}

static void copy(void *to, const void *from, uint32_t n)
{
    uint8_t *d = to;
    const uint8_t *s = from;
    while (n--)
        *d++ = *s++;
}

void track_names_init(track_names *names, uint32_t track_count)
{
    zero(names, sizeof *names);
    names->count = track_count > TRACK_NAMES_CAP ? TRACK_NAMES_CAP : track_count;
}

int track_names_load(track_names *names, const uint8_t *bytes, uint32_t len)
{
    uint32_t count = names->count;
    track_names_init(names, count);
    if (!bytes || len < 2 || bytes[0] != TRACK_NAMES_VERSION || bytes[1] != count)
        return 0;
    track_names loaded;
    track_names_init(&loaded, count);
    uint32_t at = 2;
    for (uint32_t t = 0; t < count; t++) {
        if (at >= len)
            return 0;
        uint32_t n = bytes[at++];
        if (n > TRACK_NAME_MAX || at + n > len)
            return 0;
        copy(loaded.name[t], bytes + at, n);
        at += n;
    }
    *names = loaded;
    return 1;
}

uint32_t track_names_encode(const track_names *names, uint8_t *out, uint32_t cap)
{
    uint32_t need = 2;
    for (uint32_t t = 0; t < names->count; t++)
        need += 1 + dk_text_len(names->name[t]);
    if (cap < need)
        return 0;
    out[0] = TRACK_NAMES_VERSION;
    out[1] = (uint8_t)names->count;
    uint32_t at = 2;
    for (uint32_t t = 0; t < names->count; t++) {
        uint32_t n = dk_text_len(names->name[t]);
        out[at++] = (uint8_t)n;
        copy(out + at, names->name[t], n);
        at += n;
    }
    return at;
}

const char *track_names_name_of(const track_names *names, uint32_t track)
{
    return track < names->count ? names->name[track] : "";
}

int track_names_rename(track_names *names, uint32_t track, const char *text)
{
    if (track >= names->count)
        return -1;
    uint32_t n = text ? dk_text_len(text) : 0;
    if (n > TRACK_NAME_MAX)
        n = TRACK_NAME_MAX;
    zero(names->name[track], sizeof names->name[track]);
    copy(names->name[track], text, n);
    return 0;
}
