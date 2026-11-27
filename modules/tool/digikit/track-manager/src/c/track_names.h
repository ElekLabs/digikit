/* track_names: the owner of the track names. One name per track, kept as text; the value saved in the module's settings is
 * version byte 1, track count, then per track a length byte and that many characters. Pure logic: no SDK calls. */
#ifndef TRACK_NAMES_H
#define TRACK_NAMES_H
#include <stdint.h>

#define TRACK_NAME_MAX 15  /* characters of one name */
#define TRACK_NAMES_CAP 32 /* tracks the table has room for */
#define TRACK_NAMES_VERSION 1
#define TRACK_NAMES_KEY "names" /* the settings key the saved value is kept under */
#define TRACK_NAMES_ENCODED_MAX (2 + TRACK_NAMES_CAP * (1 + TRACK_NAME_MAX))

typedef struct track_names {
    uint32_t count;
    char name[TRACK_NAMES_CAP][TRACK_NAME_MAX + 1];
} track_names;

/* No names, for `track_count` tracks (cut to the room of the table). */
void track_names_init(track_names *names, uint32_t track_count);

/* Replaces the names by the saved value. A value that is empty, has another version, has another track count than
 * `names->count` or is cut short loads as no names. Returns 1 when the value was used. */
int track_names_load(track_names *names, const uint8_t *bytes, uint32_t len);

/* Writes the value to `out`; returns its length, or 0 when `cap` is too small. */
uint32_t track_names_encode(const track_names *names, uint8_t *out, uint32_t cap);

/* The name of `track` (from 0), or "" when it has none or the track does not exist. */
const char *track_names_name_of(const track_names *names, uint32_t track);

/* Sets the name. Empty text removes it; text over TRACK_NAME_MAX characters is cut. Returns 0, or -1 for a track that does not exist. */
int track_names_rename(track_names *names, uint32_t track, const char *text);

#endif
