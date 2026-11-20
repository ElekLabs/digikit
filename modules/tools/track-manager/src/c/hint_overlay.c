/* hint_overlay: see hint_overlay.h. The box follows the active track while it is open: draw reads it every frame. Line 1 "TRACK NN" in the small font, line 2 the name
 * in the large font (or "-"). */
#include "hint_overlay.h"

#include "active_track.h"
#include "name_line.h"
#include "track_text.h"

#define STRIP_MARGIN 4
#define TEXT_PAD 4
#define TEXT_TOP 3
#define LINE_GAP 2 /* between the small line and the large one */
#define STRIP_HEIGHT (TEXT_TOP + TM_GLYPH_H + LINE_GAP + NAME_LINE_H_ROWS + TEXT_TOP)
#define STRIP_BOTTOM 4

static dk_ui *ui;
static const track_names *names;
static dk_ui_view *view;
static uintptr_t
    serial; /* which opening `view` is: `closed` of an older view must not clear a newer handle */

static void draw(void *arg, const dk_canvas *canvas)
{
    (void)arg;
    uint32_t track = active_track_or_first();
    char label[9];
    track_text_label(label, track);
    int32_t x0 = STRIP_MARGIN, x1 = (int32_t)canvas->width - 1 - STRIP_MARGIN;
    int32_t y1 = (int32_t)canvas->height - 1 - STRIP_BOTTOM, y0 = y1 - STRIP_HEIGHT + 1;
    canvas->fill_rect(x0, y0, x1, y1, 0);
    canvas->frame_rect(x0, y0, x1, y1, 1);
    canvas->text(x0 + TEXT_PAD, y0 + TEXT_TOP, label);
    name_line_draw(canvas, x0 + TEXT_PAD, y0 + TEXT_TOP + TM_GLYPH_H + LINE_GAP,
                   x1 - x0 + 1 - 2 * TEXT_PAD, track_names_name_of(names, track));
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
