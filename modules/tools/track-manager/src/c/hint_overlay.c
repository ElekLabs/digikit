/* hint_overlay: see hint_overlay.h. The strip follows the active track while it is open: draw reads it every frame. */
#include "hint_overlay.h"

#include "active_track.h"
#include "track_text.h"

#define STRIP_MARGIN 4
#define TEXT_PAD 4
#define TEXT_TOP 3
#define STRIP_HEIGHT (TM_GLYPH_H + 2 * TEXT_TOP)
#define STRIP_BOTTOM 4
#define LABEL_MAX 40

static dk_ui *ui;
static const track_names *names;
static dk_ui_view *view;
static uintptr_t
    serial; /* which opening `view` is: `closed` of an older view must not clear a newer handle */

static void append(char *out, uint32_t *at, const char *text)
{
    while (*text && *at + 1 < LABEL_MAX)
        out[(*at)++] = *text++;
    out[*at] = 0;
}

static void draw(void *arg, const dk_canvas *canvas)
{
    (void)arg;
    uint32_t track = active_track_or_first();
    const char *name = track_names_name_of(names, track);
    char number[3], label[LABEL_MAX];
    uint32_t at = 0;
    track_text_number(number, track);
    label[0] = 0;
    if (name[0]) {
        append(label, &at, number);
        append(label, &at, "  ");
        append(label, &at, name);
    } else {
        append(label, &at, "TRACK ");
        append(label, &at, number);
    }
    int32_t x0 = STRIP_MARGIN, x1 = (int32_t)canvas->width - 1 - STRIP_MARGIN;
    int32_t y1 = (int32_t)canvas->height - 1 - STRIP_BOTTOM, y0 = y1 - STRIP_HEIGHT + 1;
    canvas->fill_rect(x0, y0, x1, y1, 0);
    canvas->frame_rect(x0, y0, x1, y1, 1);
    canvas->text(x0 + TEXT_PAD, y0 + TEXT_TOP, label);
}

static void closed(void *arg)
{
    if ((uintptr_t)arg == serial)
        view = 0;
}

void hint_overlay_init(dk_ui *ui_handle, const track_names *track_names_state)
{
    ui = ui_handle;
    names = track_names_state;
    view = 0;
}

void hint_overlay_show(void *arg)
{
    (void)arg;
    if (view || !ui || active_track() < 0)
        return; /* no project: there is no track to show */
    serial++;
    const dk_ui_view_desc desc = {
        .flags = DK_UI_VIEW_OVERLAY, .draw = draw, .closed = closed, .arg = (void *)serial};
    view = dk_ui_view_open(ui, &desc);
}

void hint_overlay_hide(void *arg)
{
    (void)arg;
    if (view)
        dk_ui_view_close(view);
    view = 0;
}
