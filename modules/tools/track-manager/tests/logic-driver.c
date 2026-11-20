/* Host driver for the pure logic of the track manager: track_names and grid_cursor. Aborts on the first failed check. */
#undef NDEBUG
#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "../src/c/grid_cursor.h"
#include "../src/c/track_names.h"

#define TRACKS_A 16u
#define TRACKS_B 12u

static void names_round_trip(void)
{
    track_names n;
    track_names_init(&n, TRACKS_A);
    assert(strcmp(track_names_name_of(&n, 3), "") == 0);
    assert(track_names_rename(&n, 3, "BASS") == 0);
    assert(track_names_rename(&n, TRACKS_A - 1, "A-Z 0~9 !@#$%^&()_+-=") ==
           0); /* cut to 15, characters kept */
    uint8_t buf[TRACK_NAMES_ENCODED_MAX];
    uint32_t len = track_names_encode(&n, buf, sizeof buf);
    assert(len == 2 + TRACKS_A + 4 + TRACK_NAME_MAX);
    assert(buf[0] == 1 && buf[1] == TRACKS_A);
    assert(track_names_encode(&n, buf, len - 1) == 0);

    track_names m;
    track_names_init(&m, TRACKS_A);
    assert(track_names_load(&m, buf, len) == 1);
    assert(strcmp(track_names_name_of(&m, 3), "BASS") == 0);
    assert(strcmp(track_names_name_of(&m, TRACKS_A - 1), "A-Z 0~9 !@#$%^&") == 0);
    assert(strcmp(track_names_name_of(&m, 0), "") == 0);
    assert(strcmp(track_names_name_of(&m, TRACKS_A), "") == 0);
    assert(track_names_rename(&m, TRACKS_A, "X") == -1);
}

static void names_empty_and_remove(void)
{
    track_names n;
    track_names_init(&n, TRACKS_A);
    uint8_t buf[TRACK_NAMES_ENCODED_MAX];
    uint32_t len = track_names_encode(&n, buf, sizeof buf);
    assert(len == 2 + TRACKS_A);
    track_names_rename(&n, 5, "KICK");
    track_names_rename(&n, 5, "");
    assert(strcmp(track_names_name_of(&n, 5), "") == 0);
    assert(track_names_encode(&n, buf, sizeof buf) == len);
    track_names_rename(&n, 5, "KICK");
    track_names_rename(&n, 5, NULL);
    assert(strcmp(track_names_name_of(&n, 5), "") == 0);
}

static void names_bad_values(void)
{
    track_names n;
    track_names_init(&n, TRACKS_A);
    track_names_rename(&n, 1, "HAT");
    uint8_t buf[TRACK_NAMES_ENCODED_MAX];
    uint32_t len = track_names_encode(&n, buf, sizeof buf);

    track_names m;
    track_names_init(&m, TRACKS_A);
    uint8_t bad[TRACK_NAMES_ENCODED_MAX];
    memcpy(bad, buf, len);
    bad[0] = 2; /* other version */
    track_names_rename(&m, 0, "OLD");
    assert(track_names_load(&m, bad, len) == 0);
    assert(strcmp(track_names_name_of(&m, 0), "") == 0); /* a bad value leaves no names */
    assert(track_names_load(&m, buf, len - 1) == 0);     /* cut short */
    assert(track_names_load(&m, buf, 0) == 0);
    assert(track_names_load(&m, NULL, 0) == 0);
    memcpy(bad, buf, len);
    bad[2 + 0] = TRACK_NAME_MAX + 1; /* length over the maximum */
    assert(track_names_load(&m, bad, len) == 0);
    track_names other;
    track_names_init(&other, TRACKS_B); /* another track count */
    assert(track_names_load(&other, buf, len) == 0);
    assert(strcmp(track_names_name_of(&other, 1), "") == 0);
}

static void cursor_full_grid(void)
{
    grid_cursor c;
    grid_cursor_init(&c, TRACKS_A);
    assert(grid_cursor_rows(&c) == 2);
    assert(grid_cursor_track(&c) == 0);
    grid_cursor_move(&c, GRID_LEFT);
    assert(grid_cursor_track(&c) == TRACKS_A - 1); /* wraps over all tracks */
    grid_cursor_move(&c, GRID_RIGHT);
    assert(grid_cursor_track(&c) == 0);
    grid_cursor_move(&c, GRID_RIGHT);
    assert(grid_cursor_track(&c) == 1);
    grid_cursor_move(&c, GRID_DOWN);
    assert(grid_cursor_track(&c) == 1 + GRID_COLUMNS);
    grid_cursor_move(&c, GRID_DOWN); /* wraps between rows */
    assert(grid_cursor_track(&c) == 1);
    grid_cursor_move(&c, GRID_UP);
    assert(grid_cursor_track(&c) == 1 + GRID_COLUMNS);
    grid_cursor_jump(&c, 9);
    assert(grid_cursor_track(&c) == 9);
    grid_cursor_jump(&c, TRACKS_A); /* does not exist */
    assert(grid_cursor_track(&c) == 9);
    grid_cursor_jump(&c, GRID_COLUMNS - 1);
    grid_cursor_move(&c, GRID_RIGHT); /* end of row 0 goes to row 1 */
    assert(grid_cursor_track(&c) == GRID_COLUMNS);
}

static void cursor_short_row(void)
{
    grid_cursor c;
    grid_cursor_init(&c, TRACKS_B); /* row 0: 0..7, row 1: 8..11 */
    assert(grid_cursor_rows(&c) == 2);
    grid_cursor_jump(&c, 2);
    grid_cursor_move(&c, GRID_DOWN);
    assert(grid_cursor_track(&c) == 2 + GRID_COLUMNS);
    grid_cursor_move(&c, GRID_UP);
    assert(grid_cursor_track(&c) == 2);
    grid_cursor_jump(&c, 6); /* column 6 has no cell in row 1 */
    grid_cursor_move(&c, GRID_DOWN);
    assert(grid_cursor_track(&c) == TRACKS_B - 1);
    grid_cursor_move(&c, GRID_UP); /* column 3 of the short row */
    assert(grid_cursor_track(&c) == TRACKS_B - 1 - GRID_COLUMNS);
    grid_cursor_jump(&c, TRACKS_B - 1);
    grid_cursor_move(&c, GRID_RIGHT);
    assert(grid_cursor_track(&c) == 0);
    grid_cursor_move(&c, GRID_LEFT);
    assert(grid_cursor_track(&c) == TRACKS_B - 1);
    grid_cursor_jump(&c, 7);
    grid_cursor_move(&c, GRID_UP); /* row 0 up wraps to the short row */
    assert(grid_cursor_track(&c) == TRACKS_B - 1);
}

static void cursor_one_row(void)
{
    grid_cursor c;
    grid_cursor_init(&c, 3);
    grid_cursor_move(&c, GRID_DOWN);
    assert(grid_cursor_track(&c) == 0);
    grid_cursor_init(&c, 0);
    grid_cursor_move(&c, GRID_RIGHT);
    assert(grid_cursor_track(&c) == 0);
}

int main(void)
{
    names_round_trip();
    names_empty_and_remove();
    names_bad_values();
    cursor_full_grid();
    cursor_short_row();
    cursor_one_row();
    return 0;
}
