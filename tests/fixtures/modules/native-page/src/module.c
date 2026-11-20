/* Neutral SDK fixture: the second native page reuses two saved words.
 * LOCAL zoom is UI-only state, independent per source track. No firmware calls. */
#include <stddef.h>
#include <dk/gen/machine.h>
#include <dk/page/page.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include "module_ids.h"

static void native_draw(void *arg, const dk_page_frame *frame);
static void native_event(void *arg, const dk_page_event *event);

static int32_t zoom[16];
/* Exported measurements let the harness check state without inferring numbers from pixels. */
uint32_t native_draws, native_events, native_track;
int32_t native_start_seen, native_end_seen, native_zoom_seen, native_level_seen;

/* Read-only test observation; no parameter or UI mutations through RPC. */
static int32_t status(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    uint8_t *out = reply;
    (void)cmd;
    (void)seq;
    (void)request;
    if (len)
        return -1;
    uint32_t words[] = {native_draws,
                        native_events,
                        native_track,
                        (uint32_t)native_start_seen,
                        (uint32_t)native_end_seen,
                        (uint32_t)native_zoom_seen,
                        (uint32_t)native_level_seen};
    for (unsigned i = 0; i < 7; i++) {
        out[4 * i] = words[i] >> 24;
        out[4 * i + 1] = words[i] >> 16;
        out[4 * i + 2] = words[i] >> 8;
        out[4 * i + 3] = words[i];
    }
    return sizeof(words);
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_NATIVE, "NATIVE", "NAT");
    dk_page *knobs = dk_machine_add_page(m, "Native proof", "NAT");
    dk_param *start = dk_page_add_param(knobs, "Start", "START", 16);
    dk_param *end = dk_page_add_param(knobs, "End", "END", 112);
    dk_page *page = dk_machine_add_page(m, "Native proof", "NAT");
    dk_page_canvas(page, 0, native_draw, native_event, NULL);
    dk_page_bind(page, start, 0);
    dk_page_bind_local(page, 0, 2);
    dk_page_bind(page, end, 7);
    return dk_host_register(0x3e, 1, status);
}

const dk_module_def dk_module = {.init = init};

static void number(const dk_page_canvas_v1 *canvas, int x, int y, int value)
{
    char text[12];
    char *at = text + sizeof(text) - 1;
    *at = 0;
    unsigned mag = value < 0 ? (unsigned)-value : (unsigned)value;
    do {
        *--at = (char)('0' + mag % 10);
        mag /= 10;
    } while (mag);
    if (value < 0)
        *--at = '-';
    canvas->text(x, y, at);
}

static void native_draw(void *arg, const dk_page_frame *frame)
{
    const dk_page_canvas_v1 *canvas = &frame->canvas;
    (void)arg;
    if (frame->version != 1 || frame->size < sizeof(*frame) || frame->track >= 16)
        return;
    native_draws++;
    native_track = frame->track;
    native_start_seen = frame->params[0];
    native_end_seen = frame->params[7];
    native_zoom_seen = zoom[frame->track];
    native_level_seen = frame->level;
    canvas->text(0, 7, "LEVEL");
    number(canvas, 32, 7, frame->level);
    canvas->text(0, 18, "START");
    canvas->text(38, 18, "ZOOM");
    canvas->text(76, 18, "END");
    number(canvas, 0, 27, frame->params[0] >> 8);
    number(canvas, 42, 27, zoom[frame->track]);
    number(canvas, 76, 27, frame->params[7] >> 8);
    /* Deliberately cross the canvas edges: the adapter clips both shapes. */
    canvas->fill_rect(-12, 34, 30, 39, -1);
    canvas->fill_rect(66, 44, (int)canvas->width + 12, (int)canvas->height + 8, -1);
}

static void native_event(void *arg, const dk_page_event *event)
{
    (void)arg;
    if (event->version != 1 || event->size < sizeof(*event) || event->track >= 16 ||
        event->control != 0)
        return;
    int next = zoom[event->track] + event->delta;
    zoom[event->track] = next < 0 ? 0 : next > 127 ? 127 : next;
    native_events++;
}
