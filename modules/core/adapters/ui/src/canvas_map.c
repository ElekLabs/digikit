/* dk_canvas over a firmware drawing surface (canvas_map.h). Pure logic. */
#include "canvas_map.h"

static const ui_canvas_fw *g_fw;

/* Orders the corners and cuts them to the screen. 0 when nothing of the rectangle is on the screen. */
static int clip(const ui_canvas_fw *fw, int32_t *x0, int32_t *y0, int32_t *x1, int32_t *y1)
{
    int32_t t, w = (int32_t)fw->width, h = (int32_t)fw->height;
    if (*x0 > *x1) {
        t = *x0;
        *x0 = *x1;
        *x1 = t;
    }
    if (*y0 > *y1) {
        t = *y0;
        *y0 = *y1;
        *y1 = t;
    }
    if (*x1 < 0 || *y1 < 0 || *x0 >= w || *y0 >= h)
        return 0;
    if (*x0 < 0)
        *x0 = 0;
    if (*y0 < 0)
        *y0 = 0;
    if (*x1 >= w)
        *x1 = w - 1;
    if (*y1 >= h)
        *y1 = h - 1;
    return 1;
}

/* Top-left rows y0..y1 (ordered, on the screen) as the firmware counts rectangle rows. */
static void rows_for_fw(const ui_canvas_fw *fw, int32_t *y0, int32_t *y1)
{
    if (fw->rect_from_bottom) {
        int32_t last = (int32_t)fw->height - 1, t = last - *y1;
        *y1 = last - *y0;
        *y0 = t;
    }
}

static void c_fill(int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color)
{
    const ui_canvas_fw *fw = g_fw;
    if (!fw || !clip(fw, &x0, &y0, &x1, &y1))
        return;
    rows_for_fw(fw, &y0, &y1);
    fw->fill(fw->bitmap, x0, y0, x1, y1, color);
}

static void c_invert(int32_t x0, int32_t y0, int32_t x1, int32_t y1)
{
    const ui_canvas_fw *fw = g_fw;
    if (!fw || !clip(fw, &x0, &y0, &x1, &y1))
        return;
    rows_for_fw(fw, &y0, &y1);
    fw->invert(fw->bitmap, x0, y0, x1, y1);
}

/* The four edges as lines of one pixel, each cut to the screen on its own: an edge outside the screen is not drawn. Left and right skip the
   rows of the top and bottom edge. */
static void c_frame(int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color)
{
    int32_t t;
    if (x0 > x1) {
        t = x0;
        x0 = x1;
        x1 = t;
    }
    if (y0 > y1) {
        t = y0;
        y0 = y1;
        y1 = t;
    }
    c_fill(x0, y0, x1, y0, color);
    if (y1 > y0)
        c_fill(x0, y1, x1, y1, color);
    if (y1 - y0 >= 2) {
        c_fill(x0, y0 + 1, x0, y1 - 1, color);
        if (x1 > x0)
            c_fill(x1, y0 + 1, x1, y1 - 1, color);
    }
}

/* A font number the canvas knows: anything else is the small font. */
static int32_t font_of(dk_font font)
{
    return font == DK_FONT_LARGE ? DK_FONT_LARGE : DK_FONT_SMALL;
}

static int32_t c_text_width_in(dk_font font, const char *s)
{
    return g_fw && s ? g_fw->text_width(font_of(font), s) : 0;
}

static int32_t c_text_width(const char *s)
{
    return c_text_width_in(DK_FONT_SMALL, s);
}

static void c_text_in(int32_t x, int32_t y, dk_font font, const char *s)
{
    const ui_canvas_fw *fw = g_fw;
    int32_t w, f = font_of(font), h;
    if (!fw || !s || !*s)
        return;
    h = (int32_t)fw->font_h[f];
    w = fw->text_width(f, s);
    if (x >= (int32_t)fw->width || x + w <= 0 || y >= (int32_t)fw->height || y + h <= 0)
        return;
    fw->text(fw->bitmap, f, x, (int32_t)fw->height - y - h, s);
}

static void c_text(int32_t x, int32_t y, const char *s)
{
    c_text_in(x, y, DK_FONT_SMALL, s);
}

void ui_canvas_bind(dk_canvas *c, const ui_canvas_fw *fw)
{
    g_fw = fw;
    c->width = fw->width;
    c->height = fw->height;
    c->text = c_text;
    c->fill_rect = c_fill;
    c->invert_rect = c_invert;
    c->frame_rect = c_frame;
    c->text_width = c_text_width;
    c->text_in = c_text_in;
    c->text_width_in = c_text_width_in;
}

void ui_canvas_unbind(void)
{
    g_fw = 0;
}
