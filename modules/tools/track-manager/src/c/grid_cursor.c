/* grid_cursor: see grid_cursor.h. */
#include "grid_cursor.h"

void grid_cursor_init(grid_cursor *cursor, uint32_t track_count)
{
    cursor->count = track_count;
    cursor->track = 0;
}

uint32_t grid_cursor_rows(const grid_cursor *cursor)
{
    return (cursor->count + GRID_COLUMNS - 1) / GRID_COLUMNS;
}

void grid_cursor_move(grid_cursor *cursor, grid_dir dir)
{
    if (cursor->count == 0)
        return;
    if (dir == GRID_LEFT || dir == GRID_RIGHT) {
        cursor->track = dir == GRID_RIGHT ? (cursor->track + 1) % cursor->count
                                          : (cursor->track + cursor->count - 1) % cursor->count;
        return;
    }
    uint32_t rows = grid_cursor_rows(cursor);
    uint32_t row = cursor->track / GRID_COLUMNS, col = cursor->track % GRID_COLUMNS;
    row = dir == GRID_DOWN ? (row + 1) % rows : (row + rows - 1) % rows;
    uint32_t target = row * GRID_COLUMNS + col;
    cursor->track = target < cursor->count ? target : cursor->count - 1;
}

void grid_cursor_jump(grid_cursor *cursor, uint32_t track)
{
    if (track < cursor->count)
        cursor->track = track;
}

uint32_t grid_cursor_track(const grid_cursor *cursor)
{
    return cursor->track;
}
