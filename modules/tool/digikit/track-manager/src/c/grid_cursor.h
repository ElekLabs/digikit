/* grid_cursor: the selected track on the manager's grid: rows of GRID_COLUMNS tracks, the last row may be short. Pure logic. */
#ifndef GRID_CURSOR_H
#define GRID_CURSOR_H
#include <stdint.h>

#define GRID_SHIFT                                                                                 \
    3 /* columns are a power of two: shifts and masks, no division (the ColdFire code the packer accepts has none of 64-bit or remainder form) */
#define GRID_COLUMNS (1u << GRID_SHIFT)

typedef enum grid_dir { GRID_LEFT, GRID_RIGHT, GRID_UP, GRID_DOWN } grid_dir;

typedef struct grid_cursor {
    uint32_t count, track;
} grid_cursor;

/* Starts on track 0 of `track_count` tracks. */
void grid_cursor_init(grid_cursor *cursor, uint32_t track_count);

/* LEFT and RIGHT: one track, wrapping over all tracks. UP and DOWN: the row above or below, wrapping between rows, same column;
 * when that row is shorter, the last track of that row. */
void grid_cursor_move(grid_cursor *cursor, grid_dir dir);

/* Selects `track` (from 0); a track that does not exist is ignored. */
void grid_cursor_jump(grid_cursor *cursor, uint32_t track);

uint32_t grid_cursor_track(const grid_cursor *cursor);
uint32_t grid_cursor_rows(const grid_cursor *cursor);

#endif
