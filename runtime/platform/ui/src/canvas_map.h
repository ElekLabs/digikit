/* The one place that turns a module's `dk_canvas` calls (<dk/platform/canvas.h>: top-left origin, clipped) into the firmware's drawing calls
   (text y counted from the screen bottom; rectangle origin set by `rect_from_bottom`). No firmware call is here: ui.c gives the firmware side as a
   `ui_canvas_fw`, the host test gives a recorder (modules/core/adapters/ui/host/tests/canvas.rs).
   Clipping: a rectangle is cut to the screen; a frame draws only the edges that lie on the screen (its corners are not moved); a text whose box
   (its width, the `font_h` rows of its font) does not touch the screen is not drawn; a text that touches it is passed on whole, and the firmware cuts it. */
#ifndef DK_UI_CANVAS_MAP_H
#define DK_UI_CANVAS_MAP_H
#include <stdint.h>
#include <dk/platform/canvas.h>

typedef struct ui_canvas_fw {
    void *bitmap; /* the screen Bitmap of this frame, passed back to every call */
    uint32_t width, height;
    uint32_t font_h[2]; /* rows the glyph box of each font covers (dk_font index) */
    uint8_t
        rect_from_bottom; /* 1: the rectangle calls count y from the screen bottom like the text call */
    /* y_up: the row of the text's bottom edge, counted up from the screen bottom; `font` is a dk_font */
    void (*text)(void *bitmap, int32_t font, int32_t x, int32_t y_up, const char *s);
    void (*fill)(void *bitmap, int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color);
    void (*invert)(void *bitmap, int32_t x0, int32_t y0, int32_t x1, int32_t y1);
    int32_t (*text_width)(int32_t font, const char *s);
} ui_canvas_fw;

/* Fill `c` with the calls that draw through `fw` (which must stay valid until ui_canvas_unbind). One canvas at a time: the calls of `c` are plain
   functions without an argument, as `dk_page_canvas_v1`'s are. */
void ui_canvas_bind(dk_canvas *c, const ui_canvas_fw *fw);

/* After the draw callback returned: the calls of the canvas do nothing. */
void ui_canvas_unbind(void);

#endif
