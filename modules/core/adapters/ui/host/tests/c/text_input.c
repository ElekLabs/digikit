/* Host test of the dk_ui_text_input desc checks of core/ui (src/text_input.h, the same source ui.c links). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/text_input.h"

static void done(void *arg, const char *text)
{
    (void)arg;
    (void)text;
}

/* what: 0 a good desc, 1 no desc, 2 no done, 3 no title. */
int32_t ti_check(int what, uint32_t max_len, uint32_t screen_max)
{
    dk_ui_text_input_desc d = {"NAME", "KICK", max_len, done, 0};
    if (what == 1)
        return ui_text_input_check(0, screen_max);
    if (what == 2)
        d.done = 0;
    if (what == 3)
        d.title = 0;
    return ui_text_input_check(&d, screen_max);
}

/* The start text for `initial` (NULL: pass 0 for has_initial) cut to max_len; written to out (16 bytes). */
void ti_initial(const char *initial, int has_initial, uint32_t max_len, char *out)
{
    dk_ui_text_input_desc d = {"NAME", has_initial ? initial : 0, max_len, done, 0};
    char buf[DK_UI_TEXT_INPUT_MAX + 1];
    ui_text_input_initial(buf, &d);
    memcpy(out, buf, sizeof buf);
}

/* ---- the owner object over a fake of the firmware's naming screen ---- */
#define TAGS 4
static ui_text_input input;
static ui_defer defer;
static ui_text_fw fake;
static int done_calls[TAGS];
static char done_text[TAGS][16];
static int pushes, push_result;
static uint32_t pushed_serial;
static char pushed_initial[16];
static int opened_events, done_events;

static int unkept_events;

static void f_emit(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a)
{
    (void)fw;
    (void)owner;
    (void)a;
    if (flags != DK_LOG_F_KEEP)
        unkept_events++;
    if (kind == DK_LOG_TEXT_INPUT_OPENED)
        opened_events++;
    else if (kind == DK_LOG_TEXT_INPUT_DONE)
        done_events++;
}
int ti_events(int kind)
{
    if (kind == 2)
        return unkept_events;
    return kind ? done_events : opened_events;
}

static int32_t f_push(void *fw, const dk_ui_text_input_desc *d, const char *initial,
                      uint32_t serial)
{
    (void)fw;
    (void)d;
    pushes++;
    if (push_result)
        return push_result;
    pushed_serial = serial;
    for (int i = 0; i < 16; i++)
        pushed_initial[i] = initial[i];
    return 0;
}

static void on_done(void *arg, const char *text)
{
    int tag = (int)(intptr_t)arg;
    done_calls[tag]++;
    for (int i = 0; i < 16; i++)
        done_text[tag][i] = text[i];
}

void ti_reset(void)
{
    memset(&input, 0, sizeof input);
    memset(done_calls, 0, sizeof done_calls);
    memset(done_text, 0, sizeof done_text);
    pushes = push_result = 0;
    pushed_serial = 0;
    memset(pushed_initial, 0, sizeof pushed_initial);
    fake.fw = 0;
    fake.push = f_push;
    fake.emit = f_emit;
    opened_events = done_events = unkept_events = 0;
    memset(&defer, 0, sizeof defer);
    input.fw = &fake;
    input.defer = &defer;
}
void ti_set_push_result(int r)
{
    push_result = r;
}
int32_t ti_open(uintptr_t owner, int tag, uint32_t max_len)
{
    dk_ui_text_input_desc d = {"NAME", "KICK", max_len, on_done, (void *)(intptr_t)tag};
    return ui_text_open(&input, owner, &d, 16);
}
void ti_yes(const char *text)
{
    ui_text_done(&input, text);
}
void ti_no(void)
{
    ui_text_gone(&input, pushed_serial);
}
void ti_gone_serial(uint32_t serial)
{
    ui_text_gone(&input, serial);
}
void ti_drop(uintptr_t owner)
{
    ui_text_drop_owner(&input, owner);
}
uint32_t ti_pushed_serial(void)
{
    return pushed_serial;
}
int ti_pushes(void)
{
    return pushes;
}
int ti_done_calls(int tag)
{
    return done_calls[tag];
}
const char *ti_done_text(int tag)
{
    return done_text[tag];
}
const char *ti_pushed_initial(void)
{
    return pushed_initial;
}

/* the same call from a key callback on a press of `code` */
int32_t ti_open_in_key(uintptr_t owner, int tag, uint32_t max_len, uint32_t code)
{
    int32_t r;
    ui_defer_enter(&defer, code);
    r = ti_open(owner, tag, max_len);
    ui_defer_leave(&defer);
    return r;
}
void ti_release(uint32_t code)
{
    ui_defer_released(&defer, code);
}
static int never_down(void *k, uint32_t code)
{
    (void)k;
    (void)code;
    return 0;
}
void ti_defer_poll(void)
{
    ui_defer_poll(&defer, never_down, 0);
}
uint32_t ti_defer_pending(void)
{
    return ui_defer_pending(&defer);
}
