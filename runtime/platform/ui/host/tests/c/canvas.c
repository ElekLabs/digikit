/* Host test of the canvas mapping of core/ui (src/canvas_map.h, the same source ui.c links): a recorder stands for the firmware's drawing calls. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "../../../src/canvas_map.h"

static char log_buf[2048];
static dk_canvas canvas;
static ui_canvas_fw fw;

static void add(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
#include <stdarg.h>
static void add(const char *fmt, ...)
{
    va_list ap;
    size_t n = strlen(log_buf);
    va_start(ap, fmt);
    vsnprintf(log_buf + n, sizeof log_buf - n, fmt, ap);
    va_end(ap);
}

static void r_text(void *bm, int32_t font, int32_t x, int32_t y_up, const char *s)
{
    (void)bm;
    add("%c %d %d %s\n", font ? 'L' : 'T', (int)x, (int)y_up, s);
}
static void r_fill(void *bm, int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t c)
{
    (void)bm;
    add("F %d %d %d %d %d\n", (int)x0, (int)y0, (int)x1, (int)y1, (int)c);
}
static void r_invert(void *bm, int32_t x0, int32_t y0, int32_t x1, int32_t y1)
{
    (void)bm;
    add("I %d %d %d %d\n", (int)x0, (int)y0, (int)x1, (int)y1);
}
/* the small font is 4 pixels for a character, the large one 8 */
static int32_t r_width(int32_t font, const char *s)
{
    return (int32_t)strlen(s) * (font ? 8 : 4);
}

void cv_begin(int rect_from_bottom)
{
    log_buf[0] = 0;
    fw = (ui_canvas_fw){0};
    fw.width = 128;
    fw.height = 64;
    fw.font_h[0] = 6;
    fw.font_h[1] = 15;
    fw.rect_from_bottom = (uint8_t)rect_from_bottom;
    fw.text = r_text;
    fw.fill = r_fill;
    fw.invert = r_invert;
    fw.text_width = r_width;
    ui_canvas_bind(&canvas, &fw);
}
void cv_end(void)
{
    ui_canvas_unbind();
}
const char *cv_log(void)
{
    return log_buf;
}
uint32_t cv_width(void)
{
    return canvas.width;
}
uint32_t cv_height(void)
{
    return canvas.height;
}
void cv_text(int x, int y, const char *s)
{
    canvas.text(x, y, s);
}
void cv_fill(int x0, int y0, int x1, int y1, int c)
{
    canvas.fill_rect(x0, y0, x1, y1, c);
}
void cv_invert(int x0, int y0, int x1, int y1)
{
    canvas.invert_rect(x0, y0, x1, y1);
}
void cv_frame(int x0, int y0, int x1, int y1, int c)
{
    canvas.frame_rect(x0, y0, x1, y1, c);
}
int cv_text_width(const char *s)
{
    return canvas.text_width(s);
}
void cv_text_in(int x, int y, int font, const char *s)
{
    canvas.text_in(x, y, (dk_font)font, s);
}
int cv_text_width_in(int font, const char *s)
{
    return canvas.text_width_in((dk_font)font, s);
}
