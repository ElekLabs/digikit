/* test/ui-views: a content module that uses the module screens of core/ui (<dk/platform/ui.h>).
 *
 * init:  opens core/ui, adds two SETTINGS rows (dk_ui_settings_row) and registers the report command.
 * row 1: opens a full-screen view: a frame and a line of text, drawn on every frame. YES opens the stock text input (title NAME, start text AB, 8
 *        characters); NO closes the view; any other key is not used.
 * row 2: opens an overlay (a filled box with text). An overlay gets no keys: the command 0x0f with a body byte asks `tick` (the UI task) to close it.
 *
 *   0x0f  with no body: report; reply, big-endian words:
 *           0  result of the first dk_ui_settings_row   4  result of the second          8  1 while the full-screen view is open   12  1 while the overlay is open
 *          16  draws of the full-screen view           20  draws of the overlay          24  `closed` calls of the full-screen view   28  `closed` calls of the overlay
 *          32  `done` calls of the text input          36  key calls that were presses of the full-screen view   40  the text that `done` got last (16 bytes, NUL padded)
 *   0x0f  with one body byte 1: close the overlay (tick does it)
 */
#include <dk/core/bytes.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/ui.h>

static dk_ui *ui;
static dk_ui_view *full, *overlay;
static int32_t r_row1 = 1, r_row2 = 1;
static uint32_t draws_full, draws_overlay, closed_full, closed_overlay, done_n, key_n;
static char done_text[16];
static volatile uint32_t close_overlay;

static void draw_full(void *arg, const dk_canvas *c)
{
    (void)arg;
    draws_full++;
    c->fill_rect(0, 0, (int32_t)c->width - 1, (int32_t)c->height - 1, 0);
    c->frame_rect(0, 0, (int32_t)c->width - 1, (int32_t)c->height - 1, 1);
    c->text(4, 4, "UI VIEW");
}

static void draw_overlay(void *arg, const dk_canvas *c)
{
    (void)arg;
    draws_overlay++;
    c->fill_rect(30, 20, 97, 43, 0);
    c->frame_rect(30, 20, 97, 43, 1);
    c->text(36, 28, "OVERLAY");
}

static void text_done(void *arg, const char *text)
{
    (void)arg;
    done_n++;
    uint32_t i = 0;
    while (text[i] && i < sizeof done_text - 1) {
        done_text[i] = text[i];
        i++;
    }
    while (i < sizeof done_text)
        done_text[i++] = 0;
}

static const dk_ui_text_input_desc input = {
    .title = "NAME", .initial = "AB", .max_len = 8, .done = text_done, .arg = 0};

static int32_t key_full(void *arg, dk_key key, uint32_t flags)
{
    (void)arg;
    if (!(flags & DK_UI_KEY_DOWN))
        return 1;
    key_n++;
    if (key == DK_KEY_YES) {
        (void)dk_ui_text_input(ui, &input);
        return 1;
    }
    if (key == DK_KEY_NO) {
        dk_ui_view_close(full);
        return 1;
    }
    return 0;
}

static void closed_full_fn(void *arg)
{
    (void)arg;
    full = 0;
    closed_full++;
}

static void closed_overlay_fn(void *arg)
{
    (void)arg;
    overlay = 0;
    closed_overlay++;
}

static const dk_ui_view_desc full_desc = {
    .flags = 0, .draw = draw_full, .key = key_full, .closed = closed_full_fn, .arg = 0};
static const dk_ui_view_desc overlay_desc = {.flags = DK_UI_VIEW_OVERLAY,
                                             .draw = draw_overlay,
                                             .key = 0,
                                             .closed = closed_overlay_fn,
                                             .arg = 0};

static void open_full(void *arg)
{
    (void)arg;
    if (!full)
        full = dk_ui_view_open(ui, &full_desc);
}

static void open_overlay(void *arg)
{
    (void)arg;
    if (!overlay)
        overlay = dk_ui_view_open(ui, &overlay_desc);
}

static int32_t command(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    uint8_t *out = reply;
    (void)seq;
    if (cmd != 0x0f)
        return -50;
    if (len == 1 && ((const uint8_t *)request)[0] == 1) {
        close_overlay = 1;
        return 0;
    }
    if (len != 0)
        return -50;
    dk_bytes_put32(out + 0, (uint32_t)r_row1);
    dk_bytes_put32(out + 4, (uint32_t)r_row2);
    dk_bytes_put32(out + 8, full != 0);
    dk_bytes_put32(out + 12, overlay != 0);
    dk_bytes_put32(out + 16, draws_full);
    dk_bytes_put32(out + 20, draws_overlay);
    dk_bytes_put32(out + 24, closed_full);
    dk_bytes_put32(out + 28, closed_overlay);
    dk_bytes_put32(out + 32, done_n);
    dk_bytes_put32(out + 36, key_n);
    for (uint32_t i = 0; i < sizeof done_text; i++)
        out[40 + i] = (uint8_t)done_text[i];
    return 56;
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_ui_open(ctx, &ui);
    if (r != DK_OK)
        return r;
    r_row1 = dk_ui_settings_row(ui, "VIEW DEMO", open_full, 0);
    r_row2 = dk_ui_settings_row(ui, "OVERLAY DEMO", open_overlay, 0);
    dk_host_register(0x0f, 1, command);
    return DK_OK;
}

static void tick(dk_module_ctx *ctx)
{
    (void)ctx;
    if (close_overlay) {
        close_overlay = 0;
        dk_ui_view_close(overlay);
    }
}

const dk_module_def dk_module = {.init = init, .tick = tick};
