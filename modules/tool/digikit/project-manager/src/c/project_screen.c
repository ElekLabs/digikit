/* project_screen: see project_screen.h. Layout for 128 x 64 in the stock font: the title, the project line, the state line, a line of rows, the key line.
 * `draw` runs for every frame, so it reads the project service each time and the screen follows a save without a key. */
#include "project_screen.h"

#include "project_text.h"

#define MARGIN 4
#define GLYPH_H 6
#define LINE_GAP 6 /* between the text rows */
#define TITLE_Y 0
#define PROJECT_Y (GLYPH_H + 8)
#define STATE_Y (PROJECT_Y + GLYPH_H + LINE_GAP)
#define DATA_Y                                                                                     \
    (STATE_Y + GLYPH_H + LINE_GAP + GLYPH_H +                                                      \
     LINE_GAP) /* line 4: one row is left empty below the state */

static dk_ui *ui;
static dk_ui_view *view;

static void draw(void *arg, const dk_canvas *canvas)
{
    (void)arg;
    canvas->fill_rect(0, 0, (int32_t)canvas->width - 1, (int32_t)canvas->height - 1, 0);
    canvas->text(MARGIN, TITLE_Y, "PROJECT");
    uint32_t slot = 0;
    uint8_t name[PROJECT_NAME_BYTES];
    char title[PROJECT_LINE_MAX];
    uint32_t status = dk_project_status();
    if (dk_project_current(&slot, name) == DK_OK) {
        project_text_title(title, slot, name);
        canvas->text(MARGIN, PROJECT_Y, title);
        canvas->text(MARGIN, STATE_Y, project_text_state(status));
        canvas->text(MARGIN, DATA_Y, project_text_data(status));
    } else {
        canvas->text(MARGIN, PROJECT_Y, project_text_none());
    }
    const char *exit_text = "[NO] EXIT";
    canvas->text((int32_t)canvas->width - MARGIN - canvas->text_width(exit_text),
                 (int32_t)canvas->height - GLYPH_H - 1, exit_text);
}

static int32_t key(void *arg, dk_key k, uint32_t flags)
{
    (void)arg;
    if ((flags & DK_UI_KEY_DOWN) && k == DK_KEY_NO)
        dk_ui_view_close(view);
    return 1;
}

static void closed(void *arg)
{
    (void)arg;
    view = 0;
}

void project_screen_init(dk_ui *ui_handle)
{
    ui = ui_handle;
    view = 0;
}

void project_screen_open(void *arg)
{
    (void)arg;
    if (view || !ui)
        return;
    const dk_ui_view_desc desc = {.draw = draw, .key = key, .closed = closed};
    view = dk_ui_view_open(ui, &desc);
}
