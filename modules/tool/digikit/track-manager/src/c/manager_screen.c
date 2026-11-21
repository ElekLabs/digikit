/* manager_screen: see manager_screen.h. Layout for 128 x 64: the title and the two grid rows in the stock font (glyphs 6 pixels high; a bar under named tracks),
 * then "TRACK NN" in the small font and the name in the large font (or "-"), and the key line at the bottom; the right-aligned text uses `text_width`. */
#include "manager_screen.h"

#include <dk/platform/settings.h>

/* a chain of key tests would become a PC-relative jump table, which is not ColdFire code the packer accepts */
#pragma GCC optimize("no-jump-tables")

#include "active_track.h"
#include "grid_cursor.h"
#include "name_line.h"
#include "track_text.h"

#define MARGIN 4
#define CELL_W                                                                                     \
    14 /* two digits are at most 12 pixels wide in the stock font; the gap between cells is what is left */
#define ROW_GAP 3 /* between the glyph rows of the grid, with room for the bar under a number */
#define TITLE_Y 0
#define GRID_Y (TM_GLYPH_H + 5)     /* below the title */
#define BAR_OFFSET (TM_GLYPH_H + 1) /* from the top of the number to the bar under it */
#define SELECTED_GAP 4              /* above the "TRACK NN" line */
#define NAME_GAP 1                  /* between that line and the large name */
#define KEYS_Y_FROM_BOTTOM (TM_GLYPH_H + 1)
#define TITLE_MAX 24

static dk_ui *ui;
static track_names *names;
static dk_settings *settings;
static dk_ui_view *view;
static grid_cursor cursor;
static uint32_t editing; /* the track the naming screen is open for */

static void save(void)
{
    uint8_t value[TRACK_NAMES_ENCODED_MAX];
    uint32_t len = track_names_encode(names, value, sizeof value);
    if (settings && len)
        dk_settings_put(settings, TRACK_NAMES_KEY, value, len);
}

static void renamed(void *arg, const char *text)
{
    (void)arg;
    if (track_names_rename(names, editing, text) == 0)
        save();
}

static void open_naming(void)
{
    static char title[TITLE_MAX];
    char number[3];
    track_text_number(number, grid_cursor_track(&cursor));
    const char *head = "TRACK ", *tail = " NAME";
    uint32_t at = 0;
    for (const char *s = head; *s; s++)
        title[at++] = *s;
    for (const char *s = number; *s; s++)
        title[at++] = *s;
    for (const char *s = tail; *s; s++)
        title[at++] = *s;
    title[at] = 0;
    editing = grid_cursor_track(&cursor);
    const dk_ui_text_input_desc desc = {
        .title = title,
        .initial = track_names_name_of(names, editing),
        .max_len = TRACK_NAME_MAX,
        .done = renamed,
    };
    dk_ui_text_input(ui, &desc);
}

static void draw(void *arg, const dk_canvas *canvas)
{
    (void)arg;
    canvas->fill_rect(0, 0, (int32_t)canvas->width - 1, (int32_t)canvas->height - 1, 0);
    canvas->text(MARGIN, TITLE_Y, "TRACK MANAGER");
    for (uint32_t t = 0; t < cursor.count; t++) {
        char number[3];
        track_text_number(number, t);
        int32_t x = MARGIN + (int32_t)(t % GRID_COLUMNS) * CELL_W;
        int32_t y = GRID_Y + (int32_t)(t / GRID_COLUMNS) * (TM_GLYPH_H + ROW_GAP);
        canvas->text(x, y, number);
        int32_t w = canvas->text_width(number);
        if (track_names_name_of(names, t)[0])
            canvas->fill_rect(x, y + BAR_OFFSET, x + w - 1, y + BAR_OFFSET, 1);
        if (t == grid_cursor_track(&cursor))
            canvas->invert_rect(x - 1, y - 1, x + w, y + TM_GLYPH_H);
    }
    uint32_t track = grid_cursor_track(&cursor);
    char label[9];
    track_text_label(label, track);
    int32_t keys_y = (int32_t)canvas->height - KEYS_Y_FROM_BOTTOM;
    int32_t name_y = keys_y - SELECTED_GAP - NAME_LINE_H_ROWS;
    int32_t label_y = name_y - NAME_GAP - TM_GLYPH_H;
    canvas->text(MARGIN, label_y, label);
    name_line_draw(canvas, MARGIN, name_y, (int32_t)canvas->width - 2 * MARGIN,
                   track_names_name_of(names, track));
    const char *exit_text = "[NO] EXIT";
    canvas->text(MARGIN, keys_y, "[YES] RENAME");
    canvas->text((int32_t)canvas->width - MARGIN - canvas->text_width(exit_text), keys_y,
                 exit_text);
}

static int32_t key(void *arg, dk_key k, uint32_t flags)
{
    (void)arg;
    if (!(flags & DK_UI_KEY_DOWN))
        return 1;
    if (k >= DK_KEY_TRIG1 && k < DK_KEY_TRIG1 + cursor.count)
        grid_cursor_jump(&cursor, k - DK_KEY_TRIG1);
    else if (k == DK_KEY_LEFT)
        grid_cursor_move(&cursor, GRID_LEFT);
    else if (k == DK_KEY_RIGHT)
        grid_cursor_move(&cursor, GRID_RIGHT);
    else if (k == DK_KEY_UP)
        grid_cursor_move(&cursor, GRID_UP);
    else if (k == DK_KEY_DOWN)
        grid_cursor_move(&cursor, GRID_DOWN);
    else if (k == DK_KEY_YES)
        open_naming();
    else if (k == DK_KEY_NO)
        dk_ui_view_close(view);
    return 1;
}

static void closed(void *arg)
{
    (void)arg;
    view = 0;
}

void manager_screen_init(dk_ui *ui_handle, track_names *track_names_state,
                         dk_settings *settings_handle)
{
    ui = ui_handle;
    names = track_names_state;
    settings = settings_handle;
    view = 0;
    grid_cursor_init(&cursor, names->count);
}

void manager_screen_open(void *arg)
{
    (void)arg;
    if (view || !ui)
        return;
    grid_cursor_init(&cursor, names->count);
    grid_cursor_jump(&cursor, active_track_or_first());
    const dk_ui_view_desc desc = {.draw = draw, .key = key, .closed = closed};
    view = dk_ui_view_open(ui, &desc);
}
