/* Host test of the sample picker (sdk/src/sample_picker.c): its own source, compiled here against a fake dk_api (a listing provider and
   a 16-track sample table) and the fake <dk/ui.h> (src/lib.rs). picker.rs presses the keys. The source is included, not linked, so
   that pf_reset can clear its statics between tests. The pure functions (picker_logic.c) are linked. */
#include <stdint.h>
#include <string.h>
#include "../../../../../../../sdk/src/sample_picker.c"

#define MAXE 400

static dk_api api;
static struct {
    int n, read, finished, list_calls, cancels, client_calls;
    int32_t ticket, list_result, poll_error;
    char dir[PK_PATH];
    dk_file_entry e[MAXE];
    dk_file_event *notify;
} fl;
static struct {
    int status[TRACKS];
    dk_sample_info info[TRACKS];
    char path[TRACKS][PK_PATH];
    int select_result, selects, unbinds, last_track, path_calls;
    char last_path[PK_PATH];
} sm;
static dk_ui_menu *settings_menu_;

static int32_t f_client(void)
{
    fl.client_calls++;
    return 7;
}
static int32_t f_list(uint32_t c, uint32_t root, const uint8_t *path, dk_file_event *notify, uint8_t *ctx)
{
    (void)c;
    (void)ctx;
    fl.list_calls++;
    if (root != DK_FILES_SAMPLES)
        return -1;
    strcpy(fl.dir, (const char *)path);
    fl.n = fl.read = fl.finished = 0;
    fl.notify = notify;
    if (fl.list_result < 0)
        return fl.list_result;
    return ++fl.ticket;
}
static int32_t f_poll(uint32_t c, uint32_t t, uint8_t *out, uint32_t cap)
{
    (void)c;
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
static int32_t f_cancel(uint32_t c, uint32_t t)
{
    (void)c;
    (void)t;
    fl.cancels++;
    return 0;
}
static int32_t s_select(uint32_t h, uint32_t t, const uint8_t *path)
{
    (void)h;
    sm.selects++;
    sm.last_track = (int)t;
    strcpy(sm.last_path, (const char *)path);
    if (sm.select_result == 0) {
        strcpy(sm.path[t], (const char *)path);
        sm.status[t] = DK_SAMPLE_QUEUED;
    }
    return sm.select_result;
}
static int32_t s_unbind(uint32_t h, uint32_t t)
{
    (void)h;
    sm.unbinds++;
    sm.status[t] = 0;
    sm.path[t][0] = 0;
    return 0;
}
static int32_t s_status(uint32_t h, uint32_t t)
{
    (void)h;
    return sm.status[t];
}
static int32_t s_info(uint32_t h, uint32_t t, uint8_t *out)
{
    (void)h;
    memcpy(out, &sm.info[t], sizeof sm.info[t]);
    return 0;
}
static int32_t s_path(uint32_t h, uint32_t t, uint8_t *buf, uint32_t cap)
{
    uint32_t n = (uint32_t)strlen(sm.path[t]);
    (void)h;
    sm.path_calls++;
    if (!sm.path[t][0])
        return DK_SAMPLE_E_NOT_FOUND;
    if (n + 1 > cap)
        return DK_SAMPLE_E_SEL_PATH;
    memcpy(buf, sm.path[t], n + 1);
    return (int32_t)n;
}

static void fill_settings(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    dk_picker_open(m);
}
static const dk_ui_menu_desc settings_desc = {.name = "Settings", .title = "SHOT", .fill = fill_settings};

/* Clears every static of the picker and the fakes. with_path: the runtime has sample_track_path (api 1.6). */
void pf_reset(int with_path)
{
    memset(&fl, 0, sizeof fl);
    memset(&sm, 0, sizeof sm);
    memset(&api, 0, sizeof api);
    g_api = 0;
    g_handle = g_client = 0;
    g_has_path = 0;
    g_tracks = g_browser = g_info = 0;
    g_track = g_n = g_more = g_state = g_err = g_ticking = g_info_file = g_info_at = 0;
    g_ticket = 0;
    g_dir[0] = 0;
    memset(g_refused, 0, sizeof g_refused);
    api.version = 1;
    api.size = with_path ? sizeof(dk_api) : offsetof(dk_api, sample_track_path);
    api.files_client = f_client;
    api.files_list = f_list;
    api.files_poll = f_poll;
    api.files_cancel = f_cancel;
    api.sample_select = s_select;
    api.sample_unbind = s_unbind;
    api.sample_track_status = s_status;
    api.sample_track_info = s_info;
    api.sample_track_path = s_path;
}

void pf_short_api(void) { api.size = offsetof(dk_api, files_client); }
int pf_init(uint32_t handle) { return dk_picker_init(&api, handle); }
dk_ui_menu *pf_settings(void)
{
    settings_menu_ = dk_ui_menu_new(&settings_desc);
    return settings_menu_;
}
void pf_fini(void) { dk_picker_fini(); }

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
void pf_finish(void) /* the listing is complete: the notify callback runs, as the UI task would run it */
{
    fl.finished = 1;
    fl.notify(7, (uint32_t)fl.ticket, 0, 0);
}
void pf_partial(void) { fl.notify(7, (uint32_t)fl.ticket, 0, 0); } /* rows are ready, the end is not */
void pf_stale_notify(void) { fl.notify(7, (uint32_t)fl.ticket - 1, 0, 0); }
void pf_fail(int rc)
{
    fl.poll_error = rc;
    fl.notify(7, (uint32_t)fl.ticket, 0, 0);
}
void pf_list_result(int rc) { fl.list_result = rc; }
int pf_list_calls(void) { return fl.list_calls; }
int pf_cancels(void) { return fl.cancels; }
const char *pf_list_dir(void) { return fl.dir; }
int pf_client_calls(void) { return fl.client_calls; }

/* the fake sample table */
void pf_track(int t, int status, const char *path, uint32_t frames, uint32_t rate, uint32_t channels)
{
    sm.status[t] = status;
    strcpy(sm.path[t], path);
    sm.info[t].frames = frames;
    sm.info[t].rate = rate;
    sm.info[t].channels = channels;
}
void pf_status(int t, int status) { sm.status[t] = status; }
void pf_select_result(int rc) { sm.select_result = rc; }
int pf_selects(void) { return sm.selects; }
int pf_unbinds(void) { return sm.unbinds; }
int pf_last_track(void) { return sm.last_track; }
const char *pf_last_path(void) { return sm.last_path; }
int pf_ticking(void) { return g_ticking; }
