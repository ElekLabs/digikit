#include <stddef.h>
#include <dk/core/module.h>
#include <dk/page/page.h>
#include "module_ids.h"

enum {
    BAND_COUNT = 8,
    PLOT_WIDTH = 51,
    PLOT_HEIGHT = 17,
    PIXEL = 256,
    AXIS_END = (PLOT_WIDTH - 1) * PIXEL,
    WORD_MAX = 32512,
};

static int clamp(int value, int max)
{
    if (value < 0)
        return 0;
    return value < max ? value : max;
}

/* Log-axis half-power half-width in subpixels, at 1/16 knob intervals.
 * For Q = 0.7 * (50 / 0.7)^p the width is asinh(1 / (2 * Q)) / ln(2) octaves. */
static int half_width(int word)
{
    static const uint16_t widths[] = {1217, 958, 747, 578, 446, 343, 263, 202, 155,
                                      119,  91,  70,  53,  41,  31,  24,  18};
    int index = word * 16 / WORD_MAX;
    if (index == 16)
        return widths[index];
    int fraction = word * 16 % WORD_MAX;
    return widths[index] + (widths[index + 1] - widths[index]) * fraction / WORD_MAX;
}

/* Bilinear frequency warping squeezes the bandwidth near Nyquist.
 * The factor is sin(2*pi*f/fs) / (2*pi*f/fs) in Q8, sampled every 5 pixels. */
static int bandwidth_scale(int center)
{
    static const uint16_t scale[] = {256, 256, 256, 256, 256, 256, 255, 251, 236, 180, 28};
    int index = center / (5 * PIXEL);
    if (index == 10)
        return scale[index];
    int fraction = center % (5 * PIXEL);
    return scale[index] + (scale[index + 1] - scale[index]) * fraction / (5 * PIXEL);
}

static void draw_response(void *arg, const dk_page_frame *frame)
{
    (void)arg;
    const dk_page_canvas_v1 *canvas = &frame->canvas;
    if (!canvas->fill_rect || !canvas->width || !canvas->height || frame->word_count < 3)
        return;

    int width = canvas->width < PLOT_WIDTH ? (int)canvas->width : PLOT_WIDTH;
    int height = canvas->height < PLOT_HEIGHT ? (int)canvas->height : PLOT_HEIGHT;
    int freq = clamp(frame->word_values[0], WORD_MAX);
    int spread = clamp(frame->word_values[1], WORD_MAX);
    int q_width = half_width(clamp(frame->word_values[2], WORD_MAX));
    int center[BAND_COUNT], ticks[BAND_COUNT], inverse_width[BAND_COUNT];
    for (int band = 0; band < BAND_COUNT; band++) {
        /* log2(f/20) = log2(250) * freq + (2*band - 7) * spread,
         * with normalized knobs. Map log2(0.45 * 48000 / 20) to AXIS_END. */
        int position = (10118 * freq + 1270 * (2 * band - 7) * spread) / WORD_MAX;
        center[band] = clamp(position, AXIS_END);
        ticks[band] = (center[band] + PIXEL / 2) / PIXEL;
        int warped_width = (q_width * bandwidth_scale(center[band]) + PIXEL / 2) / PIXEL;
        inverse_width[band] = (1 << 18) / warped_width;
    }

    /* Sum the bands' power on the log axis. 1 / (1 + r*r) sampled at
     * quarter-width intervals gives a qualitative envelope without phase. */
    static const uint8_t power[] = {255, 240, 204, 163, 128, 100, 78, 63, 51, 42, 35,
                                    30,  26,  22,  19,  17,  15,  13, 12, 11, 10, 9,
                                    8,   7,   7,   6,   6,   5,   5,  5,  4,  4,  4};
    int response[PLOT_WIDTH];
    int peak = 1;
    for (int x = 0; x < PLOT_WIDTH; x++) {
        int sum = 0;
        for (int band = 0; band < BAND_COUNT; band++) {
            /* Keep each peak visible even when its bandwidth is below one pixel. */
            int distance = x * PIXEL - center[band];
            if (distance < 0)
                distance = -distance;
            int index = (distance * inverse_width[band] + (1 << 15)) >> 16;
            if (x == ticks[band])
                index = 0;
            if (index < (int)sizeof(power))
                sum += power[index];
        }
        response[x] = sum;
        if (sum > peak)
            peak = sum;
    }

    int bottom = height >= 4 ? height - 4 : height - 1;
    int previous = bottom;
    for (int x = 0; x < width; x++) {
        int y = bottom - (response[x] * bottom + peak / 2) / peak;
        int top = y, end = y;
        /* Two narrow peaks can be only two columns apart. Keep the
         * descending column open on a steep drop so the valley stays visible. */
        if (x && previous < y && y - previous <= 3)
            top = previous + 1;
        if (x && previous > y)
            end = previous - 1;
        canvas->fill_rect(x, top, x, end, 1);
        previous = y;
    }
    if (height >= 4) {
        for (int band = 0; band < BAND_COUNT; band++) {
            int x = ticks[band];
            if (x < width)
                canvas->fill_rect(x, height - 2, x, height - 1, 1);
        }
    }
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_filter *f = dk_module_ctx_add_filter(ctx, DK_FILTER_ID_RBNK, "Resonator Bank", "RBNK");
    dk_page *p = dk_filter_add_page(f);
    dk_page_add_param(p, "Spread", "SPRD", 64);
    dk_page_add_param(p, "Q", "Q", 20);
    dk_page_canvas(p, 0, draw_response, NULL, NULL);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
