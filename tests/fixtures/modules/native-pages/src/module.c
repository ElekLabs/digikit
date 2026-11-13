/* Neutral replacement pair. A's click action follows its new binding; E/H are
 * saved words, G on page one is local, and LEVEL stays independent. */
#include <stddef.h>
#include <dk/gen/machine.h>
#include <dk/page/page.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include "module_ids.h"
static void twin_zero(void *arg, const dk_page_frame *f);
static void twin_one(void *arg, const dk_page_frame *f);
static void twin_event(void *arg, const dk_page_event *e);
static uint32_t draws[2], clicks, last_page;
static int32_t seen[4], zoom;
static void twin_click(void *arg, uint32_t track)
{
    (void)arg;
    if (track < 16)
        clicks++;
}
static int32_t status(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    uint8_t *out = reply;
    (void)cmd;
    (void)seq;
    (void)request;
    if (len)
        return -1;
    uint32_t words[] = {draws[0],          draws[1],          clicks,
                        last_page,         (uint32_t)seen[0], (uint32_t)seen[1],
                        (uint32_t)seen[2], (uint32_t)seen[3], (uint32_t)zoom};
    for (unsigned i = 0; i < 9; i++) {
        out[4 * i] = words[i] >> 24;
        out[4 * i + 1] = words[i] >> 16;
        out[4 * i + 2] = words[i] >> 8;
        out[4 * i + 3] = words[i];
    }
    return sizeof(words);
}
static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_TWIN, "TWIN", "TWN");
    dk_page *knobs = dk_machine_add_page(m, "Twin", "TWN");
    dk_param *smp = dk_page_add_param(knobs, "Sample", "SMP", 0);
    dk_param_range(smp, 0, 15);
    dk_param *start = dk_page_add_param(knobs, "Start", "START", 16);
    dk_param *end = dk_page_add_param(knobs, "End", "END", 112);
    dk_param_on_click(smp, twin_click, NULL);
    dk_page *zero = dk_machine_add_page(m, "TWIN ZERO", "TWN");
    dk_page_canvas(zero, 0, twin_zero, twin_event, NULL);
    dk_page_bind(zero, smp, 0);
    dk_page_bind(zero, start, 4);
    dk_page_bind(zero, end, 7);
    dk_page *one = dk_machine_add_page(m, "TWIN ONE", "TWN");
    dk_page_canvas(one, 0, twin_one, twin_event, NULL);
    dk_page_bind(one, start, 4);
    dk_page_bind_local(one, 0, 6);
    dk_page_bind(one, end, 7);
    return dk_host_register(0x3f, 1, status);
}
const dk_module_def dk_module = {.init = init};
static void number(const dk_page_canvas_v1 *c, int x, int y, unsigned value)
{
    char text[12], *at = text + sizeof(text) - 1;
    *at = 0;
    do {
        *--at = (char)('0' + value % 10);
        value /= 10;
    } while (value);
    c->text(x, y, at);
}
static void draw(const dk_page_frame *f, unsigned page)
{
    if (f->version != 1 || f->size < sizeof(*f) || f->track >= 16 || f->word_count < 3)
        return;
    draws[page]++;
    last_page = page;
    for (unsigned i = 0; i < 3; i++)
        seen[i] = f->word_values[i];
    seen[3] = f->level;
    const dk_page_canvas_v1 *c = &f->canvas;
    c->text(0, 7, page ? "CUSTOM ONE" : "CUSTOM ZERO");
    c->text(0, 19, "SMP");
    number(c, 20, 19, (unsigned)seen[0] >> 8);
    c->text(50, 19, "LEV");
    number(c, 72, 19, (unsigned)seen[3] >> 8);
    c->text(0, 42, "START");
    c->text(50, 42, "LOCAL");
    c->text(76, 42, "END");
    number(c, 0, 51, (unsigned)seen[1] >> 8);
    number(c, 50, 51, (unsigned)zoom);
    number(c, 76, 51, (unsigned)seen[2] >> 8);
}
static void twin_zero(void *arg, const dk_page_frame *f)
{
    (void)arg;
    draw(f, 0);
}
static void twin_one(void *arg, const dk_page_frame *f)
{
    (void)arg;
    draw(f, 1);
}
static void twin_event(void *arg, const dk_page_event *e)
{
    (void)arg;
    if (e->version == 1 && e->size >= sizeof(*e) && e->track < 16 && e->control == 0)
        zoom += e->delta;
}
