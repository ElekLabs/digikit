/* Host test of the sample picker (sdk/src/sample/picker.c): its own source, compiled here against fakes of the imports (a listing provider and
   a 16-entry sample pool) and the fake <dk/platform/ui.h> (src/lib.rs). picker.rs presses the keys. The source is included, not linked, so
   that pf_reset can clear its statics between tests. The pure functions (logic.c) are linked. */
#include <stdint.h>
#include <string.h>
#include "../../../../../../../sdk/src/chooser/chooser.c"
#include "../../../../../../../sdk/src/sample/picker.c"

#define MAXE 400

static int pool_token, files_token, ui_token; /* the fake handles */
static struct {
    int n, read, finished, list_calls, cancels;
    int32_t ticket, list_result, poll_error;
    char dir[DK_CHOOSER_PATH];
    dk_file_entry e[MAXE];
    dk_files_event_fn *notify;
} fl;
static struct {
    int status[TRACKS];
    dk_sample_info info[TRACKS];
    char path[TRACKS][DK_CHOOSER_PATH];
    int set_result, sets, clears, last_index, path_calls, picks, last_pick;
    char last_path[DK_CHOOSER_PATH];
} sm;

int32_t dk_files_list(dk_files *f, uint32_t root, const char *path, dk_files_event_fn *notify,
                      void *arg)
{
    (void)f;
    (void)arg;
    fl.list_calls++;
    if (root != DK_FILES_SAMPLES)
        return -1;
    strcpy(fl.dir, path);
    fl.n = fl.read = fl.finished = 0;
    fl.notify = notify;
    if (fl.list_result < 0)
        return fl.list_result;
    return ++fl.ticket;
}
int32_t dk_files_poll(dk_files *f, uint32_t t, void *out, uint32_t cap)
{
    (void)f;
    if (t != (uint32_t)fl.ticket || cap < sizeof(dk_file_entry))
        return -1;
    if (fl.poll_error)
        return fl.poll_error;
    if (fl.read < fl.n) {
        memcpy(out, &fl.e[fl.read++], sizeof(dk_file_entry));
        return DK_FILES_ROW;
    }
    return fl.finished ? DK_FILES_END : DK_FILES_PENDING;
}
int32_t dk_files_cancel(dk_files *f, uint32_t t)
{
    (void)f;
    (void)t;
    fl.cancels++;
    return 0;
}
/* The fake runtime: the path goes into slot `index` (a filled slot is replaced; set_result < 0 refuses; index >= 16 is E_CONTRACT). */
int32_t dk_sample_pool_set(dk_sample_pool *p, uint32_t index, const char *path)
{
    (void)p;
    sm.sets++;
    strcpy(sm.last_path, path);
    sm.last_index = (int)index;
    if (sm.set_result < 0)
        return sm.set_result;
    if (index >= TRACKS)
        return DK_SAMPLE_E_CONTRACT;
    strcpy(sm.path[index], path);
    sm.status[index] = DK_SAMPLE_QUEUED;
    return 0;
}
int32_t dk_sample_pool_clear(dk_sample_pool *p, uint32_t t)
{
    (void)p;
    sm.clears++;
    sm.status[t] = 0;
    sm.path[t][0] = 0;
    return 0;
}
int32_t dk_sample_pool_status(dk_sample_pool *p, uint32_t t)
{
    (void)p;
    return sm.status[t];
}
int32_t dk_sample_pool_info(dk_sample_pool *p, uint32_t t, void *out)
{
    (void)p;
    memcpy(out, &sm.info[t], sizeof sm.info[t]);
    return 0;
}
int32_t dk_sample_pool_path(dk_sample_pool *p, uint32_t t, char *buf, uint32_t cap)
{
    uint32_t n = (uint32_t)strlen(sm.path[t]);
    (void)p;
    sm.path_calls++;
    if (!sm.path[t][0])
        return DK_SAMPLE_E_NOT_FOUND;
    if (n + 1 > cap)
        return DK_SAMPLE_E_SEL_PATH;
    memcpy(buf, sm.path[t], n + 1);
    return (int32_t)n;
}

static void on_pick(void *arg, int32_t index)
{
    (void)arg;
    sm.picks++;
    sm.last_pick = index;
}

/* Clears every static of the picker and the fakes. */
void pf_reset(void)
{
    memset(&fl, 0, sizeof fl);
    memset(&sm, 0, sizeof sm);
    g_pool = 0;
    g_ui = 0;
    g_files = 0;
    g_list = g_browser = g_finfo = 0;
    g_info = 0;
    g_d = 0;
    g_cur = g_slot = g_n = g_more = g_state = g_err = g_ticking = g_info_at = 0;
    g_ticket = 0;
    g_dir[0] = 0;
    g_pick = 0;
    g_pick_arg = 0;
}

/* The picker with its three handles; `pool`, `files` and `ui` are 0 for a handle the module did not get. */
int pf_init_with(int pool, int files, int ui)
{
    return (int)dk_sample_picker_init(pool ? (dk_sample_pool *)&pool_token : 0,
                                      files ? (dk_files *)&files_token : 0,
                                      ui ? (dk_ui *)&ui_token : 0);
}
int pf_init(uint32_t pool)
{
    return pf_init_with(pool != 0, 1, 1);
}
void pf_open_pool(int current, int with_pick)
{
    dk_sample_picker_open_pool(current, with_pick ? on_pick : 0, 0);
}
void pf_fini(void)
{
    dk_sample_picker_fini();
}

/* the fake provider */
void pf_add(const char *path, int dir, uint32_t size)
{
    dk_file_entry *e = &fl.e[fl.n++];
    memset(e, 0, sizeof *e);
    e->kind = dir ? DK_FILE_DIRECTORY : DK_FILE_FILE;
    e->flags = dir || !size ? 0 : DK_FILE_SIZE_VALID;
    e->size_bytes = size;
    strcpy((char *)e->path, path);
}
void pf_finish(
    void) /* the listing is complete: the notify callback runs, as the UI task would run it */
{
    fl.finished = 1;
    fl.notify(7, (uint32_t)fl.ticket, 0, 0);
}
void pf_partial(void)
{
    fl.notify(7, (uint32_t)fl.ticket, 0, 0);
} /* rows are ready, the end is not */
void pf_stale_notify(void)
{
    fl.notify(7, (uint32_t)fl.ticket - 1, 0, 0);
}
void pf_fail(int rc)
{
    fl.poll_error = rc;
    fl.notify(7, (uint32_t)fl.ticket, 0, 0);
}
void pf_list_result(int rc)
{
    fl.list_result = rc;
}
int pf_list_calls(void)
{
    return fl.list_calls;
}
int pf_cancels(void)
{
    return fl.cancels;
}
const char *pf_list_dir(void)
{
    return fl.dir;
}

/* the fake sample table */
void pf_track(int t, int status, const char *path, uint32_t frames, uint32_t rate,
              uint32_t channels)
{
    sm.status[t] = status;
    strcpy(sm.path[t], path);
    sm.info[t].frames = frames;
    sm.info[t].rate = rate;
    sm.info[t].channels = channels;
}
void pf_status(int t, int status)
{
    sm.status[t] = status;
}
void pf_set_result(int rc)
{
    sm.set_result = rc;
}
int pf_sets(void)
{
    return sm.sets;
}
int pf_clears(void)
{
    return sm.clears;
}
int pf_last_index(void)
{
    return sm.last_index;
}
int pf_picks(void)
{
    return sm.picks;
}
int pf_last_pick(void)
{
    return sm.last_pick;
}
const char *pf_last_path(void)
{
    return sm.last_path;
}

/* ---- a toy chooser: five slots, to test the component without the sample pool ---- */
static struct {
    int done_calls, last_done, fill_calls, last_slot, panel_entry, with_text;
    int fill_rc; /* < 0: fill fails with it; otherwise it returns the slot */
    char last_fill[DK_CHOOSER_PATH];
    dk_chooser_desc d;
} cf;

static void toy_row(void *arg, int32_t i, int32_t is_current, char *text)
{
    (void)arg;
    text[0] = is_current ? '>' : ' ';
    text[1] = 'r';
    text[2] = (char)('0' + i);
    text[3] = 0;
}
static int32_t toy_fill(void *arg, int32_t slot, const char *path)
{
    (void)arg;
    cf.fill_calls++;
    cf.last_slot = slot;
    strcpy(cf.last_fill, path);
    return cf.fill_rc < 0 ? cf.fill_rc : slot;
}
static void toy_error(void *arg, int32_t code, char *text)
{
    (void)arg;
    (void)code;
    if (cf.with_text)
        strcpy(text, "TOY FAILED");
}
static void toy_panel(void *arg, dk_ui_menu *p, int32_t entry)
{
    (void)arg;
    cf.panel_entry = entry;
    dk_ui_menu_add_action(p, "ONE", 0, 0, entry);
}
static void toy_done(void *arg, int32_t result)
{
    (void)arg;
    cf.done_calls++;
    cf.last_done = result;
}

int cf_init(void)
{
    return (int)dk_chooser_init((dk_files *)&files_token, (dk_ui *)&ui_token);
}
int cf_open(int current, int with_text)
{
    memset(&cf, 0, sizeof cf);
    cf.with_text = with_text;
    cf.d = (dk_chooser_desc){.title = "TOY",
                             .count = 5,
                             .row = toy_row,
                             .root = DK_FILES_SAMPLES,
                             .fill = toy_fill,
                             .error_text = toy_error,
                             .panel = toy_panel,
                             .done = toy_done};
    return dk_chooser_open(&cf.d, current);
}
int cf_open_without_fill(void)
{
    memset(&cf, 0, sizeof cf);
    cf.d = (dk_chooser_desc){.title = "TOY", .count = 5, .row = toy_row, .root = DK_FILES_SAMPLES};
    return dk_chooser_open(&cf.d, 0);
}
void cf_fill_result(int rc)
{
    cf.fill_rc = rc;
}
int cf_done_calls(void)
{
    return cf.done_calls;
}
int cf_last_done(void)
{
    return cf.last_done;
}
int cf_fill_calls(void)
{
    return cf.fill_calls;
}
int cf_last_slot(void)
{
    return cf.last_slot;
}
const char *cf_last_fill(void)
{
    return cf.last_fill;
}
int cf_panel_entry(void)
{
    return cf.panel_entry;
}
