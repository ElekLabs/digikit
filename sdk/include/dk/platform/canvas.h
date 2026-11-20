/* <dk/platform/canvas.h>: the drawing surface of a module's screen (`dk_ui_view_desc.draw` of <dk/platform/ui.h>). Hand-written.
   `width`, `height`, `text` and `fill_rect` have the types and the order of `dk_page_canvas_v1` (<dk/page/page.h>), so page code can take a
   `dk_canvas` later with no ABI change; the other calls come after them.

   The origin is the top-left pixel for every call: x grows to the right and y grows down, whatever the device's own text and
   rectangle conventions are. Coordinates outside the screen are clipped. Rectangle corners may come in any order and are inclusive.
   `color`: 1 sets the pixels, 0 clears them. `text` draws with the stock UI font, with y the top row of the text and x its left edge;
   the glyphs are ORed into what is there (clear the area first with `fill_rect` for a clean background).
   `text_in` and `text_width_in` take the font: DK_FONT_SMALL is the stock UI font that `text` uses; DK_FONT_LARGE is the large ROM font (a taller box, 15 rows, with A to Z, 0 to 9,
   the space and the symbols of the naming screen `~!@#$%^&()_+-=`). A device without the large font draws and measures with the small one. `y` is the top row of the glyph box of
   the font. They are the last fields of `dk_canvas`; the fields before them keep their places.
   The functions are valid only during `draw`. Do not keep the pointer or call them from another callback. Callbacks run in the UI task. */
#ifndef DK_CANVAS_H
#define DK_CANVAS_H
#include <stdint.h>

/* The fonts of `text_in` and `text_width_in`. */
typedef enum dk_font {
    DK_FONT_SMALL = 0,
    DK_FONT_LARGE = 1,
} dk_font;

typedef struct dk_canvas {
    uint32_t width, height;
    void (*text)(int32_t x, int32_t y, const char *text);
    void (*fill_rect)(int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color);
    /* Inverts the pixels inside the rectangle. */
    void (*invert_rect)(int32_t x0, int32_t y0, int32_t x1, int32_t y1);
    /* The one-pixel outline of the rectangle. */
    void (*frame_rect)(int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color);
    /* Width in pixels of `text` drawn with `text`; 0 for NULL. */
    int32_t (*text_width)(const char *text);
    /* `text` with a font; x is the left edge and y the top row of the glyph box. */
    void (*text_in)(int32_t x, int32_t y, dk_font font, const char *text);
    /* Width in pixels of `text` drawn with `font`; 0 for NULL. */
    int32_t (*text_width_in)(dk_font font, const char *text);
} dk_canvas;

#endif
