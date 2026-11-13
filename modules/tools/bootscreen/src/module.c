/* tools/bootscreen: a custom boot animation as a normal module.
 *
 * The module runs after the runtime's REC, safe-mode and boot-counter checks. `init` reads the setting `active` (a file name), reads
 * `data/tools/bootscreen/<name>` (a DKBS file, packages/dkbs), checks the whole file, and gives the firmware's intro one frame per tick
 * (`dk_intro_take`, <dk/platform/intro.h>). Any failure returns with no change: the stock intro runs. The module logs one line per outcome, from init only.
 *
 * `frame` runs on the intro task: it decodes one tick into the work buffer and calls no import. `done` runs once in the UI task
 * at the first UI pass and frees the buffer.
 *
 * `menu`: the row SCREEN shows the active name (or STOCK) at the right; it opens a list of STOCK and the files of `data/tools/bootscreen/`,
 * with a check mark on the active one. YES on a row chooses it. The choice is a setting: it takes effect at the next restart, and the page
 * says so while the setting differs from the one the boot used. */
#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/core/mem.h>
#include <dk/platform/data.h>
#include <dk/platform/files.h>
#include <dk/platform/intro.h>
#include <dk/platform/settings.h>
#include <dk/platform/ui.h>
#include "dkbs.h"
#include <dk/core/path.h>
#include <dk/core/text.h>

#define MODULE_ID "tools/bootscreen"
#define NAME_MAX_BYTES 48u /* a file name kept in the setting, NUL included */
#define PATH_MAX_BYTES 160u
#define MAX_CHOICES 8u /* files the chooser lists */
#define POLLS_PER_TICK 16u
#define LIST_PERIOD_MS 50u
#define ROW_COLS 20u   /* characters of the settings row: the full-width page holds about 21 */
#define VALUE_COLS 13u /* the value, after the label SCREEN and one space */

static dk_settings *g_settings;
static dk_files *g_files;
static dk_ui *g_ui;
static uint8_t *g_buffer; /* DKBS_FILE_MAX bytes of file, then DKBS_FRAME bytes of work */
static Dkbs g_dkbs;
static char g_booted
    [NAME_MAX_BYTES]; /* the setting at boot: a different current value waits for the restart */
static void say(const char *msg, uint32_t value)
{
    dk_log(DK_LOG_INFO, msg, value);
}

/* ---- the intro ------------------------------------------------------------------------------------------------------------------ */

/* Intro task: the tick must follow the decoder's tick, else the animation ends. */
static const uint8_t *frame(uint32_t tick, void *arg)
{
    Dkbs *s = (Dkbs *)arg;
    if (tick != s->tick)
        return 0;
    return dkbs_next(s);
}

/* Frees the file buffer. It is the intro's `done` (UI task, first pass: the intro is over) and the cleanup of a failed init. */
static void done(void *arg)
{
    (void)arg;
    if (g_buffer) {
        dk_free(g_buffer);
        g_buffer = 0;
    }
}

static int32_t init(dk_module_ctx *ctx)
{
    char name[NAME_MAX_BYTES], path[PATH_MAX_BYTES];
    int32_t n, size;
    if (dk_settings_open(ctx, &g_settings) != DK_OK || dk_files_open(ctx, &g_files) != DK_OK)
        return DK_OK;
    if (dk_ui_open(ctx, &g_ui) != DK_OK)
        g_ui = 0;
    n = dk_settings_get(g_settings, "active", name, NAME_MAX_BYTES - 1u);
    if (n > 0) {
        name[n] = 0;
        dk_text_copy(g_booted, NAME_MAX_BYTES, name);
    }
    if (n <= 0) {
        say("bootscreen: stock", 0);
        return 0;
    }
    name[n] = 0;
    if (dk_data_path(path, sizeof path, MODULE_ID, name) < 0) {
        say("bootscreen: bad name", 0);
        return 0;
    }
    g_buffer = dk_alloc(DKBS_FILE_MAX + DKBS_FRAME);
    if (!g_buffer) {
        say("bootscreen: no memory", 0);
        return 0;
    }
    size = dk_files_read(g_files, path, g_buffer, DKBS_FILE_MAX);
    if (size <= 0) {
        say("bootscreen: no file", (uint32_t)size);
        done(0);
        return 0;
    }
    if (!dkbs_open(&g_dkbs, g_buffer, (uint32_t)size, g_buffer + DKBS_FILE_MAX)) {
        say("bootscreen: bad file", (uint32_t)size);
        done(0);
        return 0;
    }
    n = dk_intro_take(frame, done, &g_dkbs, g_dkbs.total_ticks);
    if (n < 0) {
        say("bootscreen: refused", (uint32_t)n);
        done(0);
        return 0;
    }
    say("bootscreen: taken", g_dkbs.total_ticks);
    return DK_OK;
}

/* ---- the menu ------------------------------------------------------------------------------------------------------------------- */

static struct {
    uint32_t ticket, count;
    uint32_t more; /* files beyond MAX_CHOICES: the last row says so */
    char full[MAX_CHOICES][NAME_MAX_BYTES];
    char shown[MAX_CHOICES + 2u][DK_UI_ROW];
} list;
static dk_file_entry entry;
static dk_ui_menu *g_page, *g_list;

/* The active name into `out`; "" when there is none. */
static void active(char *out)
{
    int32_t n = g_settings ? dk_settings_get(g_settings, "active", out, NAME_MAX_BYTES - 1u) : -1;
    out[n > 0 ? n : 0] = 0;
}

/* ---- the chooser: a list of STOCK and the files; the check mark is the active one ------------------------------------------------ */

/* The row of the active setting in the chooser: 0 STOCK, 1.. a file, -1 a name that is not among the listed files. */
static int active_row(void)
{
    char name[NAME_MAX_BYTES];
    uint32_t i;
    active(name);
    if (!name[0])
        return 0;
    for (i = 0; i < list.count; i++)
        if (dk_text_equal(name, list.full[i]))
            return (int)i + 1;
    return -1;
}

static void list_fill(dk_ui_menu *m, void *arg)
{
    uint32_t i, rows = list.count + list.more + 1u;
    int on = active_row();
    (void)arg;
    dk_text_copy(list.shown[0], DK_UI_ROW, "STOCK");
    dk_ui_menu_add_text(m, list.shown[0]);
    for (i = 0; i < list.count; i++) {
        dk_text_copy(list.shown[i + 1u], DK_UI_ROW,
                     list.full[i]); /* display only: the full name is stored */
        dk_ui_menu_add_text(m, list.shown[i + 1u]);
    }
    if (list.more) {
        dk_text_copy(list.shown[list.count + 1u], DK_UI_ROW, "MORE ON CARD");
        dk_ui_menu_add_text(m, list.shown[list.count + 1u]);
    }
    for (i = 0; i < rows; i++)
        dk_ui_menu_check(m, (int)i, (int)i == on);
}

/* YES on a row chooses it: the check mark moves there (the list toggled it already) and the setting is saved. */
static void list_check(void *arg, int32_t row, int32_t on)
{
    uint32_t i, rows = list.count + list.more + 1u;
    int32_t rc;
    (void)arg;
    (void)on;
    if (row < 0 || (uint32_t)row >= rows)
        return;
    if ((uint32_t)row > list.count) {
        dk_ui_menu_check(g_list, row, 0);
        dk_ui_toast(g_ui, "MORE ON CARD");
        return;
    }
    if (row == 0)
        rc = dk_settings_remove(g_settings, "active");
    else
        rc = dk_settings_put(g_settings, "active", list.full[row - 1],
                             dk_text_len(list.full[row - 1]));
    if (rc < 0) {
        int back = active_row();
        for (i = 0; i < rows; i++)
            dk_ui_menu_check(g_list, (int)i, (int)i == back);
        dk_ui_toast(g_ui, "NOT SAVED");
        return;
    }
    for (i = 0; i < rows; i++)
        dk_ui_menu_check(g_list, (int)i, (int)i == row);
    /* A toast shows on the main screen only; the settings page has the same note as a row. */
    dk_ui_toast(g_ui, "ON NEXT RESTART");
    if (g_page)
        dk_ui_menu_refresh(g_page);
}

static const dk_ui_menu_desc list_desc = {.name = "BootscreenChooser",
                                          .title = "BOOT SCREEN",
                                          .flags = DK_UI_LIST,
                                          .fill = list_fill,
                                          .check = list_check};

static void show_choice(void)
{
    int on = active_row();
    if (!g_list)
        g_list = dk_ui_menu_new(g_ui, &list_desc);
    if (!g_list) {
        dk_ui_toast(g_ui, "LIST FAILED");
        return;
    }
    dk_ui_menu_select(g_list, on < 0 ? 0 : on);
    if (dk_ui_menu_open(g_list) < 0)
        dk_ui_toast(g_ui, "LIST FAILED");
}

static void poll(void *arg)
{
    uint32_t k;
    (void)arg;
    for (k = 0; k < POLLS_PER_TICK; k++) {
        int32_t r = dk_files_poll(g_files, list.ticket, &entry, sizeof entry);
        if (r == DK_FILES_PENDING)
            return;
        if (r == DK_FILES_ROW) {
            const char *name = dk_path_leaf((const char *)entry.path);
            if (entry.kind == DK_FILE_FILE && name[0] && dk_text_len(name) < NAME_MAX_BYTES) {
                if (list.count < MAX_CHOICES)
                    dk_text_copy(list.full[list.count++], NAME_MAX_BYTES, name);
                else
                    list.more = 1;
            }
            continue;
        }
        dk_ui_tick(g_ui, poll, 0, 0);
        if (r == DK_FILES_END)
            show_choice();
        else
            dk_ui_toast(g_ui, "LIST FAILED");
        return;
    }
}

static void open_chooser(void *arg, int32_t value)
{
    int32_t t;
    (void)arg;
    (void)value;
    if (!g_files || !g_settings || !g_ui) {
        dk_ui_toast(g_ui, "LIST UNAVAILABLE");
        return;
    }
    list.count = 0;
    list.more = 0;
    t = dk_files_list(g_files, DK_FILES_STORE, "data/tools/bootscreen", 0, 0);
    if (t <= 0) {
        dk_ui_toast(g_ui, "LIST FAILED");
        return;
    }
    list.ticket = (uint32_t)t;
    dk_ui_tick(g_ui, poll, 0, LIST_PERIOD_MS);
}

/* The shown value: the name without ".dkbs"; a longer one keeps its start and its end around a '~' (dk_text_trunc, as the module list does). */
static void value_text(char *out, const char *name)
{
    char v[NAME_MAX_BYTES];
    uint32_t n;
    dk_text_copy(v, sizeof v, name[0] ? name : "STOCK");
    n = dk_text_len(v);
    if (n > 5u && v[n - 5u] == '.' && (v[n - 4u] | 32) == 'd' && (v[n - 3u] | 32) == 'k' &&
        (v[n - 2u] | 32) == 'b' && (v[n - 1u] | 32) == 's')
        v[n -= 5u] = 0;
    dk_text_trunc(out, v, VALUE_COLS);
}

/* One row of ROW_COLS characters: the label at the left, the value at the right. The page title already names the module. */
static void menu(dk_module_ctx *ctx, dk_ui_menu *m)
{
    char name[NAME_MAX_BYTES], value[VALUE_COLS + 1u], text[DK_UI_ROW];
    uint32_t n, v;
    (void)ctx;
    g_page = m;
    active(name);
    value_text(value, name);
    v = dk_text_len(value);
    dk_text_copy(text, DK_UI_ROW, "SCREEN");
    for (n = dk_text_len(text); n + v < ROW_COLS; n++)
        text[n] = ' ';
    dk_text_copy(text + n, DK_UI_ROW - n, value);
    dk_ui_menu_add_action(m, text, open_chooser, 0, 0);
    if (!dk_text_equal(name, g_booted))
        dk_ui_menu_add_text(m, "ON NEXT RESTART");
}

const dk_module_def dk_module = {.init = init, .menu = menu};
