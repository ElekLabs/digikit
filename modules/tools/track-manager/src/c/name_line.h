/* name_line: a track name drawn on its own line in the large font, or "-" when the track has none. A name that is wider than the line is drawn in the small
 * font, vertically centred in the same box, so the 15 characters of a name always show. Widths come from `dk_canvas.text_width_in`. */
#ifndef NAME_LINE_H
#define NAME_LINE_H
#include <dk/platform/canvas.h>

/* Rows of the glyph box of the large font (docs/findings/155-key-event-path-module-views-and-naming-screen.md, section 5). */
#define NAME_LINE_H_ROWS 15
/* Rows of the small font's glyph box. */
#define NAME_LINE_SMALL_ROWS 6

/* Draws the name (or "-") with its box top at `y` and its left edge at `x`; `avail` is the width of the line in pixels. */
static inline void name_line_draw(const dk_canvas *canvas, int32_t x, int32_t y, int32_t avail,
                                  const char *name)
{
    const char *text = name[0] ? name : "-";
    if (canvas->text_width_in(DK_FONT_LARGE, text) <= avail)
        canvas->text_in(x, y, DK_FONT_LARGE, text);
    else
        canvas->text_in(x, y + (NAME_LINE_H_ROWS - NAME_LINE_SMALL_ROWS) / 2, DK_FONT_SMALL, text);
}

#endif
