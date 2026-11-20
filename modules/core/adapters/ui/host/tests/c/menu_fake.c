/* Host test of core/digikit-menu: the menu's own C source, compiled here with fakes of the runtime's dkrt_* calls and of
   dk_ui_stock_row. The fake <dk/platform/ui.h> (src/lib.rs) shows the screens, and modules/core/adapters/ui/host/tests/menu.rs presses the keys.
   The menu source is included, not linked, so that fk_reset can clear its statics between tests. */
/* an address of a module's menu function is 32 bits on the device; here the fake modules use 1 for fake_module_menu */
#include <stdint.h>
#include <dk/platform/ui.h>
static void fake_module_menu_call(uint32_t a, dk_ui_menu *m);
#define DK_CALL_MENU(a, c, m) (void)(c), fake_module_menu_call((a), (m))
#include "../../../../../apps/digikit-menu/src/menu.c"
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <assert.h>

/* ---- the fake store and runtime ---------------------------------------------------------------------------------------- */

static struct {
    int n;
    DkrtScan e[DKRT_SCAN_MAX];
    uint32_t arena[DKRT_SCAN_MAX]; /* bytes the entry takes in the arena */
    DkrtSelection sel;
    int scan_result, get_result;
    DkrtBootInfo boot;
    char boot_name[DKRT_SEL_MAX][28]; /* the file name the runtime holds each boot index under */
    int fx_n;
    DkrtFx fx[MAX_FXR];
    uint32_t send_slots;
    uint32_t arena_free;
    int mod_n;
    DkrtModule mod[4];
    int restarts, restart_result, set_result, selection_sets;
    DkrtSelection set_sel;
    int puts;
    char put_key[2][48];
    uint8_t put_val[2];
    int flushes;
    uint32_t blocks;
    /* the live job: the calls of dkrt_live, the scripted results, and the phase of the job */
    int live_n;
    uint32_t live_op[8];
    char live_path[8][64];
    int32_t live_queue_result[8]; /* per call: what dkrt_live answers (0: queued) */
    int32_t live_job_result[8];   /* per job: its result when it finishes */
    int live_wait;                /* dkrt_live_state calls that still say "running" */
    int live_waits;               /* the wait a new job gets */
    uint32_t live_phase;
    int live_cur;
} fk;
static int settings_runs, settings_acts;

static int ui_token;
dk_ui *dk_ui_core_handle(void)
{
    return (dk_ui *)&ui_token;
}

static dk_ui_action_fn *stock_fn;

void dk_ui_stock_row(uint8_t *stock_menu, const char *text, dk_ui_action_fn *fn, void *ctx)
{
    (void)stock_menu;
    (void)text;
    (void)ctx;
    stock_fn = fn;
}

int32_t dkrt_selection_get(DkrtSelection *out)
{
    if (fk.get_result != DKRT_OK)
        return fk.get_result;
    memcpy(out, &fk.sel, sizeof *out);
    return DKRT_OK;
}

int32_t dkrt_selection_set(const DkrtSelection *sel)
{
    fk.selection_sets++;
    memcpy(&fk.set_sel, sel, sizeof *sel);
    if (fk.set_result == DKRT_OK)
        memcpy(&fk.sel, sel,
               sizeof *sel); /* the saved selection is what dkrt_selection_get answers next */
    return fk.set_result;
}

int32_t dkrt_restart(void)
{
    fk.restarts++;
    return fk.restart_result;
}

void dkrt_boot_info(DkrtBootInfo *out)
{
    memcpy(out, &fk.boot, sizeof *out);
}

void dkrt_status(DkrtStatus *out)
{
    memset(out, 0, sizeof *out);
    out->runtime = 0x000501;
    out->arena_free = fk.arena_free;
}

void dkrt_health(DkrtHealth *out)
{
    memset(out, 0, sizeof *out);
}

uint32_t dkrt_capacity(DkrtCap *out, uint32_t max)
{
    if (max < 1)
        return 0;
    out[0].kind = DKRT_CAPK_SEND_SLOTS;
    out[0].limit = fk.send_slots;
    out[0].used = 0;
    out[0].largest = 0;
    return 1;
}

uint32_t dkrt_blocks(void)
{
    return fk.blocks;
}

/* The per-pass callbacks: up to four addresses. */
static uintptr_t polls[4];
static int poll_slots = 4; /* callbacks the registry takes */
static int poll_is_on(uintptr_t f)
{
    for (int i = 0; i < 4; i++)
        if (polls[i] == f)
            return 1;
    return 0;
}
int32_t dkrt_poll_add(uintptr_t f)
{
    if (poll_is_on(f))
        return 0;
    for (int i = 0; i < poll_slots; i++)
        if (!polls[i]) {
            polls[i] = f;
            return 0;
        }
    return DKRT_E_NOMEM;
}

void dkrt_poll_remove(uintptr_t f)
{
    for (int i = 0; i < 4; i++)
        if (polls[i] == f)
            polls[i] = 0;
}

int32_t dkrt_boot_name(uint32_t index, uint8_t *out)
{
    if (index >= DKRT_SEL_MAX)
        return DKRT_E_NOT_FOUND;
    memcpy(out, fk.boot_name[index], 28);
    return DKRT_OK;
}

/* The live job of the runtime: a finished load sets the file's loaded flag, a finished unload clears it and counts an unload. */
int32_t dkrt_live(uint32_t op, const uint8_t *path)
{
    int i = fk.live_n;
    if (i >= 8)
        return -50;
    fk.live_n++;
    if (fk.live_queue_result[i] != 0)
        return fk.live_queue_result[i];
    fk.live_op[i] = op;
    snprintf(fk.live_path[i], sizeof fk.live_path[i], "%s", (const char *)path);
    fk.live_phase = 1;
    fk.live_cur = i;
    fk.live_wait = fk.live_waits;
    return DKRT_OK;
}

void dkrt_live_state(DkrtLive *out)
{
    memset(out, 0, sizeof *out);
    out->phase = fk.live_phase;
    if (fk.live_phase == 1 && fk.live_wait > 0) {
        fk.live_wait--;
        return;
    }
    if (fk.live_phase == 1) {
        int i = fk.live_cur;
        fk.live_phase = 2;
        if (fk.live_job_result[i] == 0) {
            for (int k = 0; k < fk.n; k++) {
                if (strcmp((const char *)fk.e[k].path, fk.live_path[i]) != 0)
                    continue;
                if (fk.live_op[i] == DKRT_LIVE_LOAD)
                    fk.e[k].flags |= DKRT_SCAN_LOADED;
                else
                    fk.e[k].flags &= ~DKRT_SCAN_LOADED;
            }
            if (fk.live_op[i] == DKRT_LIVE_UNLOAD) {
                /* the module leaves the runtime's list: the modules after it move up one index */
                for (int m = 0; m < fk.mod_n; m++)
                    if (strcmp((const char *)fk.mod[m].path, fk.live_path[i]) == 0) {
                        memmove(&fk.mod[m], &fk.mod[m + 1],
                                (size_t)(fk.mod_n - m - 1) * sizeof fk.mod[0]);
                        fk.mod_n--;
                        break;
                    }
            }
            fk.boot.unloads += fk.live_op[i] == DKRT_LIVE_UNLOAD;
            fk.boot.reloads++;
        }
    }
    out->phase = fk.live_phase;
    out->result = fk.live_phase == 2 ? fk.live_job_result[fk.live_cur] : 0;
}

/* The menu owner count of the runtime for the one module of a test: what dkrt_menu_built and dkrt_menu_release leave (the release stops at 0, as the runtime's). */
static int menu_held;
void dkrt_menu_built(uintptr_t code, uintptr_t data)
{
    (void)code;
    (void)data;
    menu_held++;
}
void dkrt_menu_release(uintptr_t code, uintptr_t data)
{
    (void)code;
    (void)data;
    if (menu_held > 0)
        menu_held--;
}
int fk_menu_held(void)
{
    return menu_held;
}

void dkrt_diag(DkrtDiag *out)
{
    memset(out, 0, sizeof *out);
}

int32_t dkrt_scan(DkrtScan *out, uint32_t max)
{
    if (fk.scan_result < 0)
        return fk.scan_result;
    uint32_t n = (uint32_t)fk.n < max ? (uint32_t)fk.n : max;
    memcpy(out, fk.e, n * sizeof *out);
    return (int32_t)n;
}

uint32_t dkrt_scan_reads(void)
{
    return 0;
}

static void kb_text(uint8_t *dst, uint32_t bytes)
{
    char b[16];
    if (bytes < 1024)
        snprintf(b, sizeof b, "<1K");
    else
        snprintf(b, sizeof b, "%uK", (unsigned)(bytes / 1024));
    memcpy(dst, b, strlen(b) + 1);
}

/* The rule of the runtime in short: the checked entries are taken in list order into an empty arena; one that does not fit is
   flagged and not taken. An unchecked entry is judged against what the checked ones leave. */
int32_t dkrt_fit(const DkrtScan *entries, uint32_t n, const uint8_t *ticked, DkrtFitOut *out)
{
    (void)entries;
    memset(out, 0, sizeof *out);
    uint32_t used = 0;
    for (uint32_t i = 0; i < n; i++) {
        if (ticked[i]) {
            if (used + fk.arena[i] <= fk.arena_free)
                used += fk.arena[i];
            else
                out->rows[i].wont = 1;
        }
    }
    for (uint32_t i = 0; i < n; i++) {
        if (!ticked[i] && used + fk.arena[i] > fk.arena_free)
            out->rows[i].wont = 1;
        if (out->rows[i].wont) {
            snprintf((char *)out->rows[i].reason, DKRT_FIT_REASON, "CPU %uK > %uK FREE",
                     (unsigned)(fk.arena[i] / 1024), (unsigned)((fk.arena_free - used) / 1024));
        }
        if (fk.e[i].flags & DKRT_SCAN_NEEDS)
            kb_text(out->rows[i].mem, fk.arena[i]);
    }
    out->count = n;
    if (used)
        snprintf((char *)out->line, DKRT_FIT_LINE, "DSP 0%% CPU %u%%",
                 (unsigned)(used * 100 / fk.arena_free));
    return DKRT_OK;
}

int32_t dkrt_module(uint32_t index, DkrtModule *out)
{
    if ((int)index >= fk.mod_n)
        return DKRT_E_NOT_FOUND;
    memcpy(out, &fk.mod[index], sizeof *out);
    return DKRT_OK;
}

int32_t dkrt_fx(uint32_t index, DkrtFx *out)
{
    if ((int)index >= fk.fx_n)
        return DKRT_E_NOT_FOUND;
    memcpy(out, &fk.fx[index], sizeof *out);
    return DKRT_OK;
}

int32_t dkrt_settings_put(const uint8_t *key, const uint8_t *value, uint32_t len)
{
    (void)len;
    if (fk.puts < 2) {
        snprintf(fk.put_key[fk.puts], sizeof fk.put_key[0], "%s", (const char *)key);
        fk.put_val[fk.puts] = value[0];
    }
    fk.puts++;
    return DKRT_OK;
}

int32_t dkrt_settings_flush(void)
{
    fk.flushes++;
    return DKRT_OK;
}

int32_t dkrt_module_header(uint32_t first, DkrtModuleHeader *out)
{
    (void)first;
    out->version = 0x000100; /* version 0.1.0 */
    return DKRT_OK;
}

/* ---- control for the tests ---------------------------------------------------------------------------------------------- */

void fk_reset(void)
{
    memset(&fk, 0, sizeof fk);
    settings_runs = settings_acts = 0;
    fk.arena_free = 512 * 1024;
    stock_fn = 0;
    g_main = g_about = DK_HEALTH_MENU = g_mem = g_modules = g_info = 0;
    memset(g_set, 0, sizeof g_set);
    memset(g_set_path, 0, sizeof g_set_path);
    memset(g_set_open, 0, sizeof g_set_open);
    memset(g_set_hold, 0, sizeof g_set_hold);
    menu_held = 0;
    g_say_pending = 0;
    g_lq_state = LQ_IDLE;
    g_lq_n = g_lq_pos = g_lq_wait = 0;
    g_busy_row = -1;
    memset(polls, 0, sizeof polls);
    poll_slots = 4;
    DK_LIVE_JOBS = 0;
    DK_LIVE_RESULT = 0;
    g_safe_shown = 0;
    g_hdr = g_lead = 0;
    STORE = 0;
    g_selection_error = 0;
    NENT = 0;
    memset(TICK, 0, sizeof TICK);
}

/* A .dkm in the store: flags are DKRT_SCAN_* bits; arena is the bytes it takes; fx_slots or machines say what kind it is. */
void fk_add(const char *path, uint32_t flags, uint32_t arena, uint32_t fx_slots, uint32_t machines)
{
    DkrtScan *e = &fk.e[fk.n];
    memset(e, 0, sizeof *e);
    snprintf((char *)e->path, sizeof e->path, "%s", path);
    e->flags = flags;
    e->size = arena / 2;
    e->need_arena = arena;
    e->need_fx_slots = fx_slots;
    e->need_machine_rows = machines;
    fk.arena[fk.n] = arena;
    fk.n++;
}

void fk_select(const char *path)
{
    /* the boot loaded the selection as it was: index k holds the leaf of the path */
    const char *leaf = strrchr(path, '/');
    snprintf(fk.boot_name[fk.sel.count], 28, "%s", leaf ? leaf + 1 : path);
    snprintf((char *)fk.sel.paths[fk.sel.count], DKRT_SEL_PATH, "%s", path);
    fk.sel.count++;
}

void fk_scan_result(int r)
{
    fk.scan_result = r;
}

/* A module the runtime has loaded (dkrt_module), and its effect (dkrt_fx): allowed routes 1 master, 2 send. */
void fk_loaded(const char *path, const char *name)
{
    DkrtModule *m = &fk.mod[fk.mod_n++];
    memset(m, 0, sizeof *m);
    snprintf((char *)m->path, sizeof m->path, "%s", path);
    snprintf((char *)m->name, sizeof m->name, "%s", name);
}

/* A loaded module that has a menu function: the menu gets the fake settings entry, which adds a text row and an action row. */
static void settings_action(void *arg, int32_t value)
{
    (void)arg;
    settings_acts += value;
}
static void fake_module_menu(dk_ui_menu *m)
{
    settings_runs++;
    dk_ui_menu_add_text(m, "HELD 0");
    dk_ui_menu_add_action(m, "RELEASE", settings_action, 0, 1);
}
void fk_loaded_menu(const char *path, const char *name)
{
    fk_loaded(path, name);
    fk.mod[fk.mod_n - 1].menu =
        1; /* an address on the host is a small number: DK_CALL_MENU maps it */
}
int fk_settings_runs(void)
{
    return settings_runs;
}
int fk_settings_acts(void)
{
    return settings_acts;
}

static void fake_module_menu_call(uint32_t a, dk_ui_menu *m)
{
    assert(a == 1);
    fake_module_menu(m);
}

void fk_fx(const char *id, uint32_t allowed, uint32_t saved)
{
    DkrtFx *f = &fk.fx[fk.fx_n++];
    memset(f, 0, sizeof *f);
    f->allowed = allowed;
    f->saved = saved;
    f->running = saved;
    snprintf((char *)f->id, sizeof f->id, "%s", id);
    snprintf((char *)f->tab, sizeof f->tab, "GAN");
}

void fk_send_slots(uint32_t n)
{
    fk.send_slots = n;
}

void fk_restart_result(int r)
{
    fk.restart_result = r;
}

int fk_open_digikit(void)
{
    if (!stock_fn)
        dk_app_settings_rows(0);
    if (!stock_fn)
        return -1;
    stock_fn(0, 0);
    return g_main ? 0 : -1;
}

int fk_restarts(void)
{
    return fk.restarts;
}
int fk_selection_sets(void)
{
    return fk.selection_sets;
}
int fk_set_count(void)
{
    return (int)fk.set_sel.count;
}
const char *fk_set_path(int i)
{
    return (const char *)fk.set_sel.paths[i];
}
int fk_puts(void)
{
    return fk.puts;
}
const char *fk_put_key(int i)
{
    return fk.put_key[i];
}
int fk_put_val(int i)
{
    return fk.put_val[i];
}
int fk_flushes(void)
{
    return fk.flushes;
}
int fk_tick(int i)
{
    return TICK[i];
}

void fk_get_result(int r)
{
    fk.get_result = r;
}
void fk_apply(void)
{
    apply();
}
void fk_boot_safe(void)
{
    fk.boot.safe = 1;
    fk.boot.reason = DKRT_SAFE_COUNTER;
}
void fk_boot_read_failure(void)
{
    memset(fk.boot_name, 0, sizeof fk.boot_name); /* nothing was loaded */
    fk.boot.safe = 1;
    fk.boot.reason = DKRT_SAFE_READ;
    fk.boot.count = 1;
}

void fk_set_tick(int row, int on)
{
    set_tick(row, on);
}

/* ---- the live apply ------------------------------------------------------------------------------------------------------- */

/* Script: job i (a call of dkrt_live) is refused at the queue with q, or ends with result r; each job reads "running" for `waits` state calls. */
void fk_live_script(int i, int q, int r)
{
    fk.live_queue_result[i] = q;
    fk.live_job_result[i] = r;
}
void fk_live_waits(int n)
{
    fk.live_waits = n;
}
/* One UI-loop pass of the callback the menu registered (the fake registry holds one flag). */
void fk_poll(void)
{
    live_poll();
}
int fk_poll_on(void)
{
    return poll_is_on((uintptr_t)live_poll);
}
/* One pass of the callback that retries a dialog that could not open. */
void fk_say_poll(void)
{
    if (poll_is_on((uintptr_t)say_poll))
        say_poll();
}
/* Two dialogs at once: the second waits for the first. */
void fk_say_twice(void)
{
    say("FIRST", "1", 0);
    say("SECOND", "2", 0);
}
/* A live load that took the index of another file: index k now holds name. */
void fk_boot_rename(int k, const char *name)
{
    snprintf(fk.boot_name[k], 28, "%s", name);
}
int fk_live_calls(void)
{
    return fk.live_n;
}
int fk_live_op(int i)
{
    return (int)fk.live_op[i];
}
const char *fk_live_path(int i)
{
    return fk.live_path[i];
}
void fk_boot_counters(uint32_t unloads, uint32_t reloads)
{
    fk.boot.unloads = unloads;
    fk.boot.reloads = reloads;
}
/* The boot's result of selection index k: a load that failed with E_IMPORT and the module it needs. */
void fk_boot_needs(int k, const char *name)
{
    fk.boot.results[k] = DKRT_E_IMPORT;
    fk.boot.states[k] = DKRT_STATE_FAILED;
    snprintf((char *)fk.boot.needs[k], DKRT_NEEDS_LEN, "%s", name);
    if ((uint32_t)k >= fk.boot.count)
        fk.boot.count = (uint32_t)k + 1;
}
void fk_boot_count(uint32_t n)
{
    fk.boot.count = n;
}
uintptr_t fk_set_handle(int k)
{
    return (uintptr_t)g_set[k];
}
int fk_busy_row(void)
{
    return g_busy_row;
}
void fk_poll_slots(int n)
{
    poll_slots = n;
}

/* The settings pages: one pass of the callback that releases a page the user left; whether a slot is open; the module leaves the runtime; the page is filled again. */
void fk_settings_poll(void)
{
    if (poll_is_on((uintptr_t)settings_poll))
        settings_poll();
}
int fk_set_open(int k)
{
    return g_set_open[k];
}
void fk_unload_module(const char *path)
{
    for (int m = 0; m < fk.mod_n; m++)
        if (strcmp((const char *)fk.mod[m].path, path) == 0) {
            memmove(&fk.mod[m], &fk.mod[m + 1], (size_t)(fk.mod_n - m - 1) * sizeof fk.mod[0]);
            fk.mod_n--;
            return;
        }
}
void fk_set_refresh(int k)
{
    dk_ui_menu_refresh(g_set[k]);
}
