/* The on-device sample picker (docs/design/sample-picker.md): the chooser (sdk/src/chooser/chooser.c) filled with the 16 entries of a sampler
   machine's sample pool as a slot list, plus the pool's side panel (CLEAR, INFO) and the INFO page of an entry. The pure functions are in logic.c.
   Everything here runs in the UI task, so the statics need no lock. */
#include <dk/chooser/chooser.h>
#include <dk/platform/files.h>
#include <dk/sample/picker.h>
#include <dk/sample/pool.h>
#include <dk/chooser/logic.h>
#include <dk/core/path.h>
#include <dk/core/text.h>

#define TRACKS DK_SAMPLE_POOL_ENTRIES

static dk_sample_pool *g_pool;
static dk_ui *g_ui;
static dk_ui_menu *g_info; /* the INFO page of a pool entry */
static int g_info_at;
static char g_path[DK_CHOOSER_PATH]; /* scratch: a pool path */
static dk_sample_picker_pick_fn *g_pick;
static void *g_pick_arg;

static void pool_row(void *arg, int32_t t, int32_t is_current, char *text);
static int32_t pool_busy(void *arg);
static int32_t pool_fill(void *arg, int32_t slot, const char *path);
static void pool_error(void *arg, int32_t code, char *text);
static void pool_panel(void *arg, dk_ui_menu *p, int32_t t);
static void pool_done(void *arg, int32_t slot);
static void fill_info(dk_ui_menu *m, void *arg);

static const dk_ui_menu_desc info_desc = {
    .name = "SampleInfo", .title = "INFO", .flags = DK_UI_TEXT_PAGE, .fill = fill_info};

/* The slots 0 to 15 as the SMP knob numbers them. */
static const dk_chooser_desc pool_desc = {.title = "SAMPLES",
                                          .count = (int32_t)TRACKS,
                                          .row = pool_row,
                                          .busy = pool_busy,
                                          .root = DK_FILES_SAMPLES,
                                          .fill = pool_fill,
                                          .error_text = pool_error,
                                          .panel = pool_panel,
                                          .done = pool_done};

static const char *entry_path(int t)
{
    g_path[0] = 0;
    if (dk_sample_pool_path(g_pool, (uint32_t)t, g_path, sizeof g_path) < 0)
        g_path[0] = 0;
    return g_path;
}

/* One entry: "NN NAME S 1.2s", the error text, or "-" for an empty slot. */
static void pool_row(void *arg, int32_t t, int32_t is_current, char *text)
{
    int32_t st = dk_sample_pool_status(g_pool, (uint32_t)t);
    dk_sample_info info = {0, 0, 0, 0};
    (void)arg;
    if (st == DK_SAMPLE_READY && dk_sample_pool_info(g_pool, (uint32_t)t, &info) < 0)
        info.frames = info.rate = info.channels = 0;
    dk_sample_pool_row_text(text, t, is_current, st,
                            st == DK_SAMPLE_UNUSED ? "" : dk_path_leaf(entry_path(t)), info.frames,
                            info.rate, info.channels);
}

static int32_t pool_busy(void *arg)
{
    int32_t t;
    (void)arg;
    for (t = 0; t < (int32_t)TRACKS; t++) {
        int32_t st = dk_sample_pool_status(g_pool, (uint32_t)t);
        if (st == DK_SAMPLE_QUEUED || st == DK_SAMPLE_LOADING)
            return 1;
    }
    return 0;
}

/* YES on a file: the pool loads it into the slot (a filled slot is replaced). The result is the slot. */
static int32_t pool_fill(void *arg, int32_t slot, const char *path)
{
    int32_t rc;
    (void)arg;
    rc = dk_sample_pool_set(g_pool, (uint32_t)slot, path);
    return rc < 0 ? rc : slot;
}

static void pool_error(void *arg, int32_t code, char *text)
{
    char info[32];
    int i;
    (void)arg;
    dk_sample_pool_err_info(info, code);
    for (i = 0; info[i] && i < DK_UI_TOAST - 1; i++)
        text[i] = info[i];
    text[i] = 0;
}

/* The flow finished with a loaded slot: the machine sets its SMP value to it. */
static void pool_done(void *arg, int32_t slot)
{
    (void)arg;
    if (g_pick)
        g_pick(g_pick_arg, slot);
}

static void clear_cb(void *arg, int32_t t)
{
    (void)arg;
    dk_sample_pool_clear(g_pool, (uint32_t)t);
    dk_chooser_refresh();
}

static void info_cb(void *arg, int32_t t)
{
    (void)arg;
    g_info_at = t;
    dk_ui_menu_open(g_info);
}

/* RIGHT on a slot: CLEAR (one press, no confirm) and INFO. */
static void pool_panel(void *arg, dk_ui_menu *p, int32_t t)
{
    (void)arg;
    dk_ui_menu_add_action(p, "CLEAR", clear_cb, 0, t);
    dk_ui_menu_add_action(p, "INFO", info_cb, 0, t);
}

/* ---- the INFO page ---- */

static void number_line(dk_ui_menu *m, const char *a, uint32_t v, const char *b)
{
    char num[12], row[DK_UI_ROW];
    dk_text_utoa(num, v);
    dk_text_cat(row, DK_CHOOSER_TEXT + 1, a, num);
    dk_text_cat(row, DK_CHOOSER_TEXT + 1, row, b);
    dk_ui_menu_add_text(m, row);
}

static void fill_info(dk_ui_menu *m, void *arg)
{
    char row[DK_UI_ROW], num[12], text[32], nm[DK_CHOOSER_TEXT + 1];
    int32_t t = g_info_at, st = dk_sample_pool_status(g_pool, (uint32_t)t);
    dk_sample_info info = {0, 0, 0, 0};
    (void)arg;
    dk_text_utoa(num, (uint32_t)t);
    dk_text_cat(row, DK_CHOOSER_TEXT + 1, "SAMPLE ", num);
    dk_ui_menu_add_text(m, row);
    if (st == DK_SAMPLE_UNUSED) {
        dk_ui_menu_add_text(m, "EMPTY");
        return;
    }
    if (entry_path(t)[0]) {
        dk_text_trunc(nm, dk_path_leaf(g_path), 16);
        dk_ui_menu_add_text(m, nm);
        dk_chooser_wrap(m, g_path);
    }
    if (st == DK_SAMPLE_READY && dk_sample_pool_info(g_pool, (uint32_t)t, &info) >= 0) {
        number_line(m, info.channels == 2 ? "STEREO " : "MONO ", info.rate, " HZ");
        number_line(m, "", info.frames, " FRAMES");
        dk_text_secs(text, info.frames, info.rate);
        dk_text_cat(row, DK_CHOOSER_TEXT + 1, "LENGTH ", text);
        dk_ui_menu_add_text(m, row);
    }
    if (st < 0) {
        dk_sample_pool_err_info(text, st);
        dk_ui_menu_add_text(m, "STATE");
        dk_chooser_wrap(m, text);
    } else
        dk_ui_menu_add_text(m, st == DK_SAMPLE_QUEUED    ? "STATE QUEUED"
                               : st == DK_SAMPLE_LOADING ? "STATE LOADING"
                                                         : "STATE READY");
}

/* ---- the module's calls ---- */

int32_t dk_sample_picker_init(dk_sample_pool *pool, dk_files *files, dk_ui *ui)
{
    int32_t r;
    g_pool = 0;
    g_ui = 0;
    if (!pool || !files || !ui)
        return DK_E_MISSING;
    r = dk_chooser_init(files, ui);
    if (r != DK_OK)
        return r;
    g_pool = pool;
    g_ui = ui;
    return DK_OK;
}

void dk_sample_picker_open_pool(int32_t current, dk_sample_picker_pick_fn *pick, void *arg)
{
    if (!g_pool)
        return;
    g_pick = pick;
    g_pick_arg = arg;
    if (!g_info)
        g_info = dk_ui_menu_new(g_ui, &info_desc);
    if (g_info)
        dk_chooser_open(&pool_desc, current);
}

void dk_sample_picker_fini(void)
{
    dk_chooser_fini();
    g_pool = 0;
    g_ui = 0;
    g_pick = 0;
}
