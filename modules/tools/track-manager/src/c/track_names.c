/* track_names: see track_names.h. */
#include "track_names.h"

#include <string.h>

void track_names_init(track_names *names, uint32_t track_count)
{
    memset(names, 0, sizeof *names);
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
        memcpy(loaded.name[t], bytes + at, n);
        at += n;
    }
    *names = loaded;
    return 1;
}

uint32_t track_names_encode(const track_names *names, uint8_t *out, uint32_t cap)
{
    uint32_t need = 2;
    for (uint32_t t = 0; t < names->count; t++)
        need += 1 + (uint32_t)strlen(names->name[t]);
    if (cap < need)
        return 0;
    out[0] = TRACK_NAMES_VERSION;
    out[1] = (uint8_t)names->count;
    uint32_t at = 2;
    for (uint32_t t = 0; t < names->count; t++) {
        uint32_t n = (uint32_t)strlen(names->name[t]);
        out[at++] = (uint8_t)n;
        memcpy(out + at, names->name[t], n);
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
    uint32_t n = text ? (uint32_t)strlen(text) : 0;
    if (n > TRACK_NAME_MAX)
        n = TRACK_NAME_MAX;
    memset(names->name[track], 0, sizeof names->name[track]);
    memcpy(names->name[track], text, n);
    return 0;
}
