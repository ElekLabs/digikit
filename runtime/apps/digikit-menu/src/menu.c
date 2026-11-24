/* core/digikit-menu: SETTINGS > DIGIKIT, written in C on <dk/platform/ui.h> (core/ui draws it with the firmware's own views).
 *
 * DIGIKIT is a menu: ABOUT, HEALTH and MEMORY (text pages), and MODULES (a list). MODULES has one
 * row per .dkm file of the store. The check mark of a row means "loaded"; it starts from the saved selection.
 * RIGHT opens the side panel (LOAD or UNLOAD, SETTINGS... for a loaded module that has a menu function, INFO).
 * The check mark is the only on/off of a module. Nothing is saved until NO leaves the list with
 * changes pending and APPLY n CHANGES? is answered with YES. The change then runs live: the selection is saved, the unloads are queued
 * and then the loads (dkrt_live, one job at a time; the row of the running job reads LOADING or UNLOADING), and the list is scanned again
 * after each job. A job the runtime refuses or that fails ends the batch with "<NAME> NOT APPLIED" and RESTART TO APPLY?. A restart is
 * direct only to leave safe mode (APPLY & RESTART?).
 *
 * The runtime does the work of the old menu: dkrt_scan (the files, their needs), dkrt_fit (what fits next to the checked ones)
 * and the selection and settings calls. This file keeps the pending state and builds the screens.
 *
 * The globals without static are read by the proofs (tools/coldfire/digikit_menu_proof.py and the pk_digikit_* tools) by name.
 * The module links without memcpy and memset: no struct copies, no aggregate initialisers of large objects. */
#include <stdint.h>
#include <stddef.h>
#include <dk/platform/ui.h>
#include <dk_ui_core.h>
#include <dk/gen/limits.h>
#include "dkrt.h"

#define MAX_ENT DKRT_SCAN_MAX
#define NAME_COLS 12

/* ---- state read by the proofs ---------------------------------------------------------------------------------------- */

/* 0 = not read, 1 = ok, 2 = no store, 3 = read error, 4 = bad store */
uint32_t STORE;
uint32_t NENT;
/* 1 when the file's header says this runtime or device cannot load it. */
uint8_t NOK[MAX_ENT];
/* 1 when the runtime has the file loaded in this boot, 2 while its DSP part still loads. */
uint8_t LOADED[MAX_ENT];
/* The pending selection: 1 when the row is checked. It starts as the saved selection. */
uint8_t TICK[MAX_ENT];
/* Diagnostic previews (first 23 bytes of each label) and row counts. */
uint8_t DK_LOAD[40][24];
uint32_t DK_LOAD_N;
uint8_t DK_ABOUT[24][24];
uint32_t DK_ABOUT_N;
uint8_t DK_MEM[8][24];
uint32_t DK_MEM_N;
uint8_t DK_LINES[12][24];
/* The last dialog the menu opened: title, text and how many were opened. */
uint8_t DK_POPUP[48];
uint8_t DK_POPUP_TEXT[48];
uint32_t DK_POPUPS;
/* The sector reads of the last scan (the cost of opening MODULES). */
uint32_t DK_NEEDS_READS;
/* What the last APPLY did: the results of dkrt_selection_set and dkrt_restart (which returns only when it refuses), and how often it ran. */
int32_t DK_APPLY_SET;
int32_t DK_APPLY_RESTART;
uint32_t DK_APPLIES;
/* The live apply: the jobs queued so far, and the last failure (result of dkrt_live or of the job; 0 when none). */
uint32_t DK_LIVE_JOBS;
int32_t DK_LIVE_RESULT;
/* Changes pending as the last NO counted them. */
uint32_t DK_PENDING;
/* The rows of the last side panel as text. */
uint8_t DK_PANEL[4][24];
uint32_t DK_PANEL_N;
DkrtHealth DK_HEALTH;
/* The HEALTH page's menu (a core/ui object: its Menu view is the pointer at +4); the health proof reads the view's own flags. */
dk_ui_menu *DK_HEALTH_MENU;
uint32_t DK_REDRAWS;
uint32_t DK_POLLS;

/* ---- private state --------------------------------------------------------------------------------------------------- */

static DkrtScan g_ent[MAX_ENT];
static DkrtFitOut g_fit;
static DkrtSelection g_sel;
static DkrtBootInfo g_boot;
static uint8_t g_saved[MAX_ENT];
static int8_t g_sel_of
    [MAX_ENT]; /* the boot index the runtime holds the row's file name at (dkrt_boot_name; the boot's results and needs are by that index), -1: none */
static uint32_t g_saved_extra;
static int32_t g_selection_error;
static int g_info_row;
static uint32_t g_safe_shown, g_seen, g_last, g_calls;
static char g_empty_text[24];
static int
    g_hdr; /* 1: the MODULES list starts with the safe-mode header, so row r of the list is module r - 1 */
static int
    g_lead; /* 1: the rows start with a mark column, ticked as " " and a selected module that is not loaded as "-" */

static dk_ui_menu *g_main, *g_about, *g_mem, *g_modules, *g_info;
static dk_ui *g_ui; /* core/ui's handle (dk_ui_core_handle), set at the first open */

/* The settings menu of each loaded module that has a menu function, by the module's index in dkrt_module order. Each is built the first
   time the user opens it and kept. The page holds no row of the module once it is left (the module's menu function runs only while the page is open), so this menu keeps no pointer into the module's code then, and while it is open it holds the module (g_set_hold: dkrt_menu_built, then dkrt_menu_release), so the runtime does not unload or reload the module under its rows;
   a menu object that the module builds itself (dk_ui_menu_new in its menu function) is counted by core/ui (dkrt_menu_built) and the runtime refuses to unload or reload that module (E_RELOAD_MENUS). The index of a module changes when another module is unloaded, so the cache is dropped then (panel_settings). */
#define MAX_SET 16
#define NAME_ROW 32 /* a module name (31 characters at most) without its author */
/* Calls the module's menu function (dk_module.menu) at address a (DkrtModule.menu) with the module's ctx (DkrtModule.ctx) and the menu to fill: menu(ctx, menu).
   The host tests, where an address is wider than 32 bits, define their own. */
#ifndef DK_CALL_MENU
#define DK_CALL_MENU(a, c, m)                                                                      \
    ((void (*)(void *, dk_ui_menu *))(uintptr_t)(a))((void *)(uintptr_t)(c), (m))
#endif
/* The cache slot of a module's settings menu is named by the module's store path (g_set_path), not by its index in dkrt_module order, which moves when
   another module is unloaded. A menu is never freed: a slot is built once. */
static dk_ui_menu *g_set[MAX_SET];
static uint8_t g_set_path[MAX_SET][64];
static uint8_t g_set_open
    [MAX_SET]; /* 1 while the page was opened and not yet released: only then does its fill call the module */
/* The address of the module's `menu` function that the open page holds (dkrt_menu_built), 0: none. While the page is open its rows point into the module's code, so the
   runtime refuses to unload or reload the module (E_RELOAD_MENUS); settings_release gives the hold back when the page is left. */
static uintptr_t g_set_hold[MAX_SET];
static char g_set_title[MAX_SET][NAME_ROW];
static dk_ui_menu_desc g_set_desc[MAX_SET];

/* The batch of a live apply: the rows to unload, then the rows to load (g_lq_op: DKRT_LIVE_UNLOAD or DKRT_LIVE_LOAD). */
#define LQ_IDLE 0
#define LQ_START 1         /* queue the job of g_lq_pos */
#define LQ_RUN 2           /* wait for it */
#define LQ_BUSY_PASSES 600 /* UI passes a queue may wait for the runtime's one job slot */
static uint8_t
    g_lq_path[MAX_ENT]
             [64]; /* the store paths of the batch: a row index is not stable across a rescan */
static uint8_t g_lq_op[MAX_ENT];
static uint32_t g_lq_n, g_lq_pos, g_lq_wait;
static uint8_t g_lq_state;
static int g_busy_row =
    -1; /* the row whose job runs: its text reads LOADING or UNLOADING (WAITING while the runtime's job slot is taken) */
#define BOOT_NAME_LEN 28 /* bytes of dkrt_boot_name's output */

/* ---- text ------------------------------------------------------------------------------------------------------------ */

typedef struct txt {
    char b[DK_UI_LIST_TEXT];
    int n;
} txt;

static void t_init(txt *t)
{
    t->n = 0;
    t->b[0] = 0;
}

static txt *t_c(txt *t, char c)
{
    if (c && t->n < (int)sizeof t->b - 1) {
        t->b[t->n++] = c;
        t->b[t->n] = 0;
    }
    return t;
}

static txt *t_s(txt *t, const char *s)
{
    while (*s)
        t_c(t, *s++);
    return t;
}

static txt *t_b(txt *t, const uint8_t *s, int max)
{
    for (int i = 0; i < max && s[i]; i++)
        t_c(t, (char)s[i]);
    return t;
}

/* The name of a missing module as the runtime keeps it (usb-bulk), in upper case. */
static txt *t_needs(txt *t, const uint8_t *s)
{
    for (int i = 0; i < DKRT_NEEDS_LEN && s[i]; i++)
        t_c(t, (char)(s[i] >= 'a' && s[i] <= 'z' ? s[i] - 32 : s[i]));
    return t;
}

static txt *t_num(txt *t, uint32_t v)
{
    char tmp[10];
    int k = 0;
    do {
        tmp[k++] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    while (k)
        t_c(t, tmp[--k]);
    return t;
}

static txt *t_int(txt *t, int32_t v)
{
    if (v < 0) {
        t_c(t, '-');
        return t_num(t, (uint32_t)(-v));
    }
    return t_num(t, (uint32_t)v);
}

/* The list font is not monospace: a letter, a digit, '-' or '_' takes 4 pixels, a space 3, '.' 2 and '~' 5 (measured on the DT2 1.16 list
   font). Character 0x01 draws nothing and takes 4 pixels, so it pads a column the width of one letter. */
#define PAD '\x01'

static int px_of(const char *s)
{
    int w = 0;
    for (; *s; s++)
        w += *s == ' ' ? 3 : *s == '.' ? 2 : *s == '~' ? 5 : 4;
    return w;
}

/* Spaces are three pixels wide and PAD is four. */
static void t_pad_px(txt *t, int px)
{
    int rest = px - px_of(t->b);
    while (rest >= 3 && rest % 4) {
        t_c(t, ' ');
        rest -= 3;
    }
    for (; rest >= 4; rest -= 4)
        t_c(t, PAD);
}

static char upper(char c)
{
    return c >= 'a' && c <= 'z' ? (char)(c - 32) : c;
}

static char lower(char c)
{
    return c >= 'A' && c <= 'Z' ? (char)(c + 32) : c;
}

/* Copies a row's text into a 24-byte proof array. */
static void keep(uint8_t *dst, const char *s)
{
    int n = 0;
    while (n < 23 && s[n]) {
        dst[n] = (uint8_t)(s[n] == PAD ? ' ' : s[n]);
        n++;
    }
    while (n < 24)
        dst[n++] = 0;
}

static void copy_text(uint8_t *dst, int max, const char *s)
{
    int n = 0;
    while (n < max - 1 && s[n]) {
        dst[n] = (uint8_t)s[n];
        n++;
    }
    while (n < max)
        dst[n++] = 0;
}

/* ---- the runtime's result texts -------------------------------------------------------------------------------------- */

/* A short text (upper case, at most 14 characters) for each runtime result code, one table per owner range: the loader's -1..-19,
   the registry's -20..-29, the DSP loader's -30..-39, boot-time loading's -40..-49. A code without text shows as ERROR n. */
static const char *const text_runtime[20] = {
    "OK",
    "NOT FOUND",
    "BAD STORE",
    "READ ERROR",
    "BAD SHA",
    "BAD FORMAT",
    "RUNTIME OLD",
    "NO IMPORT",
    "NO MEMORY",
    "INIT FAILED",
    "ALREADY LOADED",
    "SLOT TAKEN",
    "BUSY",
    "TOO BIG",
    "TABLE FULL",
    "",
    "",
    "",
    "",
    "",
};
static const char *const text_registry[10] = {
    "NO REGISTRY", "OTHER DEVICE", "BAD TYPE", "TYPE USED", "NO PARAM ROW",
    "NO PAGE ROW", "TYPE IN USE",  "BAD ID",   "OVER USES", "FX ID TWICE",
};
static const char *const text_dsp[10] = {
    "NO DSP LINK",
    "BAD DSP CODE",
    "DSP IMPORT",
    "NO DSP SPACE",
    "NO VOICE ROW",
    "DSP XFER",
    "OTHER DEVICE",
    "BAD VOICE TYPE",
    "",
    "",
};
static const char *const text_boot[10] = {
    "BAD PATH", "TOO MANY",     "WRITE FAILED", "CARD BUSY",     "SAFE MODE",
    "",         "OUT OF RANGE", "READ ONLY",    "OTHER RUNTIME", "TABLE BAD",
};
/* The boot plan and the declarations: -73..-79 (E_MISSING, E_DECL_FULL, E_PLAN_KIND, E_PLAN_OWNER, E_PLAN_KEY, E_PLAN_FIT, E_DECL_KIND). */
static const char *const text_plan[7] = {
    "NO SERVICE", "TOO MANY CALLS", "NO OWNER", "TWO OWNERS", "KEY TWICE", "NO FIT", "BAD KIND",
};

/* The live job's refusals, -83..-87 (E_RELOAD_PLAYING, E_RELOAD_MENUS, E_RELOAD_ROUTED, E_RELOAD_KEYS, E_RELOAD_RETAINED). */
static const char *const text_live[5] = {
    "STOP PLAYBACK", "HAS MENUS", "ON AN FX ROUTE", "KEYS CHANGED", "RESTART DEVICE",
};

static void status_text(txt *t, int32_t code)
{
    uint32_t k = code < 0 ? (uint32_t)(-code) : (uint32_t)code;
    const char *known = "";
    if (code >= 0)
        known = "";
    else if (k < 20)
        known = text_runtime[k];
    else if (k < 30)
        known = text_registry[k - 20];
    else if (k < 40)
        known = text_dsp[k - 30];
    else if (k < 50)
        known = text_boot[k - 40];
    else if (k == 53) /* E_STATE of the protocol: an upload is open */
        known = "UPLOAD OPEN";
    else if (k >= 73 && k < 80)
        known = text_plan[k - 73];
    else if (k >= 83 && k < 88)
        known = text_live[k - 83];
    if (!known[0]) {
        t_s(t, "ERROR ");
        t_int(t, code);
    } else {
        t_s(t, known);
    }
}

/* The short name of a CAPACITY kind (DKRT_CAPK_*), for "REFUSED FULL <name>": at most 10 characters. */
static const char *const fit_label[17] = {
    "",          "MODULES",    "SELECTION",  "CPU MEMORY", "FILE",       "DSP MEMORY",
    "MACH ROWS", "VOICE ROWS", "PARAM ROWS", "PAGE ROWS",  "FX SLOTS",   "MAP",
    "STORE",     "SETTINGS",   "SEND SLOTS", "DSP LOAD",   "DSP VOICES",
};

/* The text of a module's state and result at boot (the runtime's DkrtBootInfo.states, .results, .needs and .detail of that selection index):
   OK, LOADING (the DSP part is still going), UNLOADED (taken down at run time), REFUSED <reason> (the boot plan refused it; E_PLAN_FIT names the resource that is full), NEEDS <module>
   when the load failed because a module it imports from is not in the image, or the code's text. A refusal has no "- " lead: the row is 23 characters. */
static void result_text(txt *t, uint32_t state, int32_t code, const uint8_t *needs,
                        const uint32_t *detail)
{
    if (state == DKRT_STATE_REFUSED) {
        t_s(t, "REFUSED ");
        if (code == DKRT_E_PLAN_FIT && detail[0] < 17) {
            t_s(t, "FULL ");
            t_s(t, fit_label[detail[0]]);
        } else {
            status_text(t, code);
        }
        return;
    }
    if (state == DKRT_STATE_UNLOADED) {
        t_s(t,
            "- UNLOADED"); /* taken down at run time: its result word is 0, which must not read OK */
        return;
    }
    t_s(t, "- ");
    if (code == 1)
        t_s(t, "LOADING");
    else if (code == 0)
        t_s(t, "OK");
    else if (code == DKRT_E_IMPORT && needs[0]) {
        t_s(t, "NEEDS ");
        t_needs(t, needs);
    } else
        status_text(t, code);
}

static void safe_reason(txt *t, uint32_t reason)
{
    // a table, not an if chain: six dense values become a jump table, which is data in the code
    static const char *const why[7] = {"UNKNOWN",     "2 BAD BOOTS", "KEY HELD",   "REQUESTED",
                                       "DSP STOPPED", "CPU PANIC",   "READ FAILED"};
    _Static_assert(DKRT_SAFE_COUNTER == 1 && DKRT_SAFE_KEY == 2 && DKRT_SAFE_FLAG == 3 &&
                       DKRT_SAFE_DSP == 4 && DKRT_SAFE_PANIC == 5 && DKRT_SAFE_READ == 6,
                   "why[] follows the DKRT_SAFE_* values");
    t_s(t, reason < 7 ? why[reason] : why[0]);
}

/* The file name of a path, upper case, at most max characters: from the end, or with head the first ones. */
static void name_of(const uint8_t *p, int len, int max, int head, txt *t)
{
    int start = 0;
    for (int k = 0; k < len; k++)
        if (p[k] == '/')
            start = k + 1;
    int n = len - start;
    if (n > max) {
        if (!head)
            start += n - max;
        n = max;
    }
    for (int k = 0; k < n; k++)
        t_c(t, upper((char)p[start + k]));
}

static int path_len(const uint8_t *p, int max)
{
    int n = 0;
    while (n < max && p[n])
        n++;
    return n;
}

static int stem_len(const uint8_t *p)
{
    int l = path_len(p, 63);
    if (l > 4 && p[l - 4] == '.' && lower((char)p[l - 3]) == 'd' && lower((char)p[l - 2]) == 'k' &&
        lower((char)p[l - 1]) == 'm')
        l -= 4;
    return l;
}

/* Short titles keep both ends of the module name. */
static void path_name(const uint8_t *p, int max, txt *t)
{
    txt full;
    t_init(&full);
    name_of(p, stem_len(p), 63, 1, &full);
    if (full.n <= max) {
        t_s(t, full.b);
        return;
    }
    int tail = max / 2 - 1, head = max - 1 - tail;
    for (int k = 0; k < head; k++)
        t_c(t, full.b[k]);
    t_c(t, '~');
    t_s(t, full.b + full.n - tail);
}

static void row_name(int i, int max, txt *t)
{
    path_name(g_ent[i].path, max, t);
}

/* ---- dialogs --------------------------------------------------------------------------------------------------------- */

/* A dialog that cannot open because another one is open waits and opens on a later UI pass (say_poll), instead of being dropped. One waits at most. */
#define SAY_PASSES 3000
static char g_say_title[48], g_say_text[48];
static dk_ui_confirm_fn *g_say_done;
static uint8_t g_say_pending;
static uint32_t g_say_wait;

static void say_poll(void);

static void say_try(void)
{
    if (dk_ui_confirm(g_ui, g_say_title, g_say_text, g_say_done, 0) == 0) {
        copy_text(DK_POPUP, sizeof DK_POPUP, g_say_title);
        copy_text(DK_POPUP_TEXT, sizeof DK_POPUP_TEXT, g_say_text);
        DK_POPUPS++;
        g_say_pending = 0;
    }
}

static void say_poll(void)
{
    if (g_say_pending && ++g_say_wait < SAY_PASSES)
        say_try();
    if (!g_say_pending || g_say_wait >= SAY_PASSES) {
        g_say_pending = 0;
        dkrt_poll_remove((uintptr_t)say_poll);
    }
}

static void say(const char *title, const char *text, dk_ui_confirm_fn *done)
{
    copy_text((uint8_t *)g_say_title, sizeof g_say_title, title);
    copy_text((uint8_t *)g_say_text, sizeof g_say_text, text);
    g_say_done = done;
    g_say_pending = 1;
    g_say_wait = 0;
    say_try();
    if (g_say_pending && dkrt_poll_add((uintptr_t)say_poll) != DKRT_OK)
        g_say_pending = 0;
}

static void fail_text(const char *what, int32_t code)
{
    txt t;
    t_init(&t);
    status_text(&t, code);
    say(what, t.b, 0);
}

/* ---- boot state ------------------------------------------------------------------------------------------------------ */

static void read_boot(void)
{
    dkrt_boot_info(&g_boot);
}

/* ---- the module store and the pending selection ---------------------------------------------------------------------- */

static int same_text(const uint8_t *a, const uint8_t *b, int max);

/* The saved selection, matched against the rows: a checked row for each path the store still has. */
static void load_selection(void)
{
    g_selection_error = dkrt_selection_get(&g_sel);
    if (g_selection_error != DKRT_OK) {
        STORE = 3;
        return;
    }
    for (uint32_t i = 0; i < MAX_ENT; i++)
        TICK[i] = 0;
    g_saved_extra = 0;
    uint32_t n = g_sel.count > DKRT_SEL_MAX ? DKRT_SEL_MAX : g_sel.count;
    for (uint32_t k = 0; k < n; k++) {
        const uint8_t *p = g_sel.paths[k];
        int l = path_len(p, DKRT_SEL_PATH - 1);
        int found = 0;
        for (uint32_t i = 0; i < NENT; i++) {
            const uint8_t *q = g_ent[i].path;
            if (path_len(q, 63) != l)
                continue;
            int same = 1;
            for (int c = 0; c < l; c++)
                same &= q[c] == p[c];
            if (same) {
                TICK[i] = 1;
                found = 1;
            }
        }
        if (!found)
            g_saved_extra++;
    }
    for (uint32_t i = 0; i < MAX_ENT; i++)
        g_saved[i] = TICK[i];
}

/* Is the file name `name` (a boot name: the leaf of a store path, 27 characters at most, NUL padded) the leaf of the path p? */
static int leaf_is(const uint8_t *p, const uint8_t *name)
{
    int start = 0;
    for (int k = 0; k < 63 && p[k]; k++)
        if (p[k] == '/')
            start = k + 1;
    for (int c = 0; c < BOOT_NAME_LEN - 1; c++) {
        if (p[start + c] != name[c])
            return 0;
        if (!name[c])
            return 1;
    }
    return 1;
}

/* g_sel_of: the runtime holds each boot index under a file name (dkrt_boot_name), which follows a live load and unload; the rows are matched by it. */
static void map_boot_names(void)
{
    for (uint32_t i = 0; i < MAX_ENT; i++)
        g_sel_of[i] = -1;
    uint32_t n = g_boot.count > DKRT_SEL_MAX ? DKRT_SEL_MAX : g_boot.count;
    for (uint32_t k = 0; k < n; k++) {
        uint8_t nm[BOOT_NAME_LEN];
        if (dkrt_boot_name(k, nm) != DKRT_OK || !nm[0])
            continue;
        for (uint32_t i = 0; i < NENT; i++)
            if (leaf_is(g_ent[i].path, nm))
                g_sel_of[i] = (int8_t)k;
    }
}

static int same_text(const uint8_t *a, const uint8_t *b, int max)
{
    for (int i = 0; i < max; i++) {
        if (a[i] != b[i])
            return 0;
        if (!a[i])
            return 1;
    }
    return 1;
}

/* Scans the store: the runtime lists the .dkm files under modules/ with their header verdict, needs and loaded state. */
static void scan_store(void)
{
    NENT = 0;
    for (uint32_t i = 0; i < MAX_ENT; i++) {
        TICK[i] = 0;
        g_saved[i] = 0;
        NOK[i] = 0;
        LOADED[i] = 0;
    }
    g_saved_extra = 0;
    int32_t n = dkrt_scan(g_ent, MAX_ENT);
    if (n < 0) {
        STORE = n == DKRT_E_NOT_FOUND ? 2 : n == DKRT_E_READ ? 3 : 4;
        return;
    }
    NENT = (uint32_t)n;
    STORE = 1;
    for (uint32_t i = 0; i < NENT; i++) {
        uint32_t f = g_ent[i].flags;
        if (!(f & DKRT_SCAN_COMPAT))
            NOK[i] = 1;
        if (f & DKRT_SCAN_LOADING)
            LOADED[i] = 2;
        else if (f & DKRT_SCAN_LOADED)
            LOADED[i] = 1;
    }
    DK_NEEDS_READS = dkrt_scan_reads();
    load_selection();
    map_boot_names();
}

/* Which modules fit next to the checked ones: the runtime judges them. */
static void refit(void)
{
    dkrt_fit(g_ent, NENT, TICK, &g_fit);
}

static int wont(int i)
{
    return g_fit.rows[i].wont != 0;
}

/* Changes since the saved state: rows checked or unchecked. */
static uint32_t changes(void)
{
    uint32_t n = 0;
    for (uint32_t i = 0; i < NENT; i++)
        n += TICK[i] != g_saved[i];
    return n;
}

static int restart_pending(void)
{
    return changes() > 0 || g_boot.safe != 0;
}

/* ---- the DIGIKIT menu ------------------------------------------------------------------------------------------------ */

static void safe_notice(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    txt t;
    t_init(&t);
    safe_reason(&t, g_boot.reason);
    say("SAFE MODE", t.b, 0);
}

static void open_health(void *ctx, int32_t arg);
static void open_modules(void *ctx, int32_t arg);

static void fill_main(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    if (g_boot.safe != 0) {
        txt t;
        t_init(&t);
        t_s(&t, "SAFE: ");
        safe_reason(&t, g_boot.reason);
        dk_ui_menu_add_action(m, t.b, safe_notice, 0, 0);
    }
    dk_ui_menu_add_submenu(m, "ABOUT", g_about);
    dk_ui_menu_add_action(m, "HEALTH", open_health, 0, 0);
    dk_ui_menu_add_submenu(m, "MEMORY", g_mem);
    dk_ui_menu_add_action(m, "MODULES", open_modules, 0, 0);
}

/* ---- ABOUT ----------------------------------------------------------------------------------------------------------- */

/* Is the file that holds the boot name nm a build with --trace? */
static int traced_name(const uint8_t *nm)
{
    for (uint32_t i = 0; i < NENT; i++)
        if (leaf_is(g_ent[i].path, nm))
            return (g_ent[i].flags & DKRT_SCAN_TRACE) != 0;
    return 0;
}

static void arow(dk_ui_menu *m, const char *s)
{
    if (DK_ABOUT_N < 24)
        keep(DK_ABOUT[DK_ABOUT_N++], s);
    dk_ui_menu_add_text(m, s);
}

static void fill_about(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    DK_ABOUT_N = 0;
    /* the per-module TRACE tag reads the scan of the store: scan once when MODULES has not (a later scan of MODULES resets its ticks, so not again here) */
    if (STORE == 0)
        scan_store();
    arow(m, "DIGIKIT 0.1.0");
    DkrtStatus st;
    dkrt_status(&st);
    read_boot();
    txt t;
    t_init(&t);
    t_s(&t, "RUNTIME ");
    t_num(&t, st.runtime >> 16);
    t_c(&t, '.');
    t_num(&t, st.runtime >> 8 & 255);
    t_c(&t, '.');
    t_num(&t, st.runtime & 255);
    arow(m, t.b);
    DkrtDiag d;
    dkrt_diag(&d);
    /* a card the runtime does not write: why, and what that switches off */
    if (d.card_state == DKRT_CARD_FOREIGN || d.card_state == DKRT_CARD_UNREADABLE) {
        arow(m, "CARD READ-ONLY");
        t_init(&t);
        if (d.card_state == DKRT_CARD_FOREIGN) {
            t_s(&t, "NOT OURS AT ");
            t_num(&t, d.card_first_nonblank);
        } else {
            t_s(&t, "CARD UNREADABLE");
        }
        arow(m, t.b);
        arow(m, "NO BOOT COUNTER");
    }
    if (d.dsp_stopped != 0) {
        arow(m, "DSP STOPPED");
        if (d.dsp_crumb != 0) {
            /* the DSP's last breadcrumb: the step, the voice and the machine type (finding 48) */
            static const char *const step[8] = {"?",    "ENTER", "FOUND", "STOCK",
                                                "CALL", "RET",   "COPY",  "DONE"};
            uint32_t s = d.dsp_crumb & 0xff;
            t_init(&t);
            t_s(&t, "LAST: ");
            t_s(&t, s < 8 ? step[s] : "?");
            t_s(&t, " V");
            t_num(&t, (d.dsp_crumb >> 8) & 0xff);
            t_s(&t, " T");
            t_num(&t, d.dsp_crumb >> 16);
            arow(m, t.b);
        }
    }
    /* the last fault (a CPU crash or a DSP stop), kept on the card: FAULT: CPU CRASH or FAULT: DSP STOP, IN <module>, PC or the DSP site and step */
    for (uint32_t k = 0; k < d.fault_rows && k < DKRT_FAULT_ROWS; k++)
        arow(m, (const char *)d.fault_text[k]);
    if (g_boot.safe != 0) {
        t_init(&t);
        t_s(&t, "SAFE: ");
        safe_reason(&t, g_boot.reason);
        arow(m, t.b);
    }
    t_init(&t);
    t_s(&t, "BOOT: ");
    t_num(&t, g_boot.loaded);
    t_s(&t, " OF ");
    t_num(&t, g_boot.count);
    t_s(&t, " OK");
    arow(m, t.b);
    /* each boot index with the name the runtime holds it under (dkrt_boot_name) and its result: a live load keeps the index of its file or takes a free
       one, so a module loaded since the boot is listed and a name never meets another module's state */
    if (dkrt_selection_get(&g_sel) != DKRT_OK)
        arow(m, "SELECTION READ ERROR");
    uint32_t n = g_boot.count > DKRT_SEL_MAX ? DKRT_SEL_MAX : g_boot.count;
    for (uint32_t k = 0; k < n; k++) {
        uint8_t nm[BOOT_NAME_LEN];
        if (dkrt_boot_name(k, nm) != DKRT_OK || !nm[0])
            continue;
        t_init(&t);
        name_of(nm, path_len(nm, BOOT_NAME_LEN - 1), 17, 0, &t);
        arow(m, t.b);
        t_init(&t);
        result_text(&t, g_boot.states[k], g_boot.results[k], g_boot.needs[k], g_boot.detail[k]);
        if (traced_name(nm))
            t_s(&t, " TRACE");
        arow(m, t.b);
    }
    t_init(&t);
    t_s(&t, "FREE ");
    t_num(&t, st.arena_free / 1024);
    t_s(&t, " KB");
    arow(m, t.b);
    arow(m, d.ram_kept != 0 ? "RAM KEPT: YES" : "RAM KEPT: NO");
}

/* ---- MEMORY ---------------------------------------------------------------------------------------------------------- */

/* CPU is the module arena alone: the system package has its own block (CAPK_PACKAGE) and is not in it, so the row reads 0% with no modules.
   The resources the page shows, in order: the CAPACITY kind, the row label and 1 when the row shows a percent (bytes), 0 for used/limit.
   Left out: the staging buffer, the selection, the effect slots and the map entries (they say nothing a user can act on). */
static const uint8_t mem_kind[7] = {
    DKRT_CAPK_DSP_AREA,   DKRT_CAPK_ARENA,      DKRT_CAPK_MODULES,  DKRT_CAPK_MACHINE_ROWS,
    DKRT_CAPK_VOICE_ROWS, DKRT_CAPK_PARAM_ROWS, DKRT_CAPK_PAGE_ROWS};
static const char *const mem_label[7] = {"DSP   ", "CPU   ", "MODS  ", "MACH  ",
                                         "VOICE ", "PARAM ", "PAGE  "};
static const uint8_t mem_pct[7] = {1, 1, 0, 0, 0, 0, 0};

/* One MEMORY row: the label, a ten-character bar (| used, . free: both are 2 pixels wide, so the bar keeps its width; # is 6 pixels
   and a full bar runs off the row), then a percent or used/limit. */
static void mem_row(dk_ui_menu *m, const char *label, const DkrtCap *e, int pct)
{
    uint32_t used = e->used > e->limit ? e->limit : e->used;
    uint32_t full = used * 10 / e->limit;
    txt t;
    t_init(&t);
    t_s(&t, label);
    for (uint32_t k = 0; k < 10; k++)
        t_c(&t, k < full ? '|' : '.');
    t_c(&t, ' ');
    if (pct) {
        t_num(&t, used * 100 / e->limit);
        t_c(&t, '%');
    } else {
        t_num(&t, e->used);
        t_c(&t, '/');
        t_num(&t, e->limit);
    }
    if (DK_MEM_N < 8)
        keep(DK_MEM[DK_MEM_N++], t.b);
    dk_ui_menu_add_text(m, t.b);
}

static void fill_mem(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    DK_MEM_N = 0;
    DkrtCap caps[DKRT_CAP_MAX];
    uint32_t n = dkrt_capacity(caps, DKRT_CAP_MAX);
    for (int r = 0; r < 7; r++)
        for (uint32_t k = 0; k < n; k++)
            /* a resource the device does not have (limit 0) has no row */
            if (caps[k].kind == mem_kind[r] && caps[k].limit != 0)
                mem_row(m, mem_label[r], &caps[k], mem_pct[r]);
}

/* ---- HEALTH (finding 41) --------------------------------------------------------------------------------------------- */

/* The page refreshes when this many DSP blocks passed (half a second), or every 60 UI passes without the block clock. */
#define H_BLOCKS 750

/* n bytes as 12.3 MB (tenths) or, below a megabyte, 511 KB. */
static void size_text(txt *t, uint32_t n)
{
    if (n >= 1024 * 1024) {
        uint32_t tenth = (n / 1024 * 10) / 1024;
        t_num(t, tenth / 10);
        t_c(t, '.');
        t_num(t, tenth % 10);
        t_s(t, " MB");
    } else {
        t_num(t, n / 1024);
        t_s(t, " KB");
    }
}

/* A size in KiB as 812 KB, 12.3 MB or 21.9 GB (the store figures: a store can be 22 GiB). */
static void size_text_kb(txt *t, uint32_t kb)
{
    if (kb >= 1024 * 1024) {
        uint32_t tenth = (kb / 1024 * 10) / 1024;
        t_num(t, tenth / 10);
        t_c(t, '.');
        t_num(t, tenth % 10);
        t_s(t, " GB");
    } else if (kb >= 1024) {
        uint32_t tenth = (kb * 10) / 1024;
        t_num(t, tenth / 10);
        t_c(t, '.');
        t_num(t, tenth % 10);
        t_s(t, " MB");
    } else {
        t_num(t, kb);
        t_s(t, " KB");
    }
}

/* A load in tenths of a percent as 37%. */
static void pct_text(txt *t, uint32_t x10)
{
    t_num(t, (x10 + 5) / 10);
    t_c(t, '%');
}

/* The next free row of the proof array DK_LINES; the page has up to 12 rows (the two BIG rows, the largest free block, only show when it differs from the total; the page is about 16 characters wide). */
static int g_hrows;

static void hrow(dk_ui_menu *m, const txt *t)
{
    if (g_hrows < 12)
        keep(DK_LINES[g_hrows++], t->b);
    dk_ui_menu_add_text(m, t->b);
}

static void fill_health(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    const DkrtHealth *h = &DK_HEALTH;
    txt t;
    for (int r = 0; r < 12; r++)
        keep(DK_LINES[r], "");
    g_hrows = 0;
    t_init(&t);
    /* relative to the quietest window since boot, not to the CPU's capacity */
    t_s(&t, "CPU REL ");
    if (h->flags & DKRT_HEALTH_CPU)
        pct_text(&t, h->cpu_x10);
    else
        t_s(&t, "--");
    hrow(m, &t);
    /* the DSP rows: only on a device with a DSP (DKRT_HEALTH_NO_DSP is the device file's capability) */
    if (!(h->flags & DKRT_HEALTH_NO_DSP)) {
        t_init(&t);
        /* the DSP block handler's share of one audio block, taking a 1 GHz DSP clock (finding 100) */
        t_s(&t, "DSP LOAD ");
        if (h->flags & DKRT_HEALTH_DSP)
            pct_text(&t, h->dsp_avg_x10);
        else
            t_s(&t, "--");
        hrow(m, &t);
        t_init(&t);
        t_s(&t, "DSP MAX ");
        if (h->flags & DKRT_HEALTH_DSP)
            pct_text(&t, h->dsp_max_x10);
        else
            t_s(&t, "--");
        hrow(m, &t);
    }
    t_init(&t);
    t_s(&t, "RAM FREE ");
    if (h->flags & DKRT_HEALTH_HEAP) {
        if (h->flags & DKRT_HEALTH_HEAP_CUT)
            t_c(&t, '>');
        size_text(&t, h->heap_free);
    } else {
        t_s(&t, "--");
    }
    hrow(m, &t);
    t_init(&t);
    t_s(&t, "LARGEST ");
    if (h->flags & DKRT_HEALTH_HEAP)
        size_text(&t, h->heap_largest);
    else
        t_s(&t, "--");
    hrow(m, &t);
    t_init(&t);
    t_s(&t, "CPU FREE ");
    size_text(&t, h->arena_free);
    hrow(m, &t);
    /* arena_largest is a payload (block size minus the 16-byte block header, DK_ARENA_HDR); arena_free sums block sizes */
    if (h->arena_largest + DK_ARENA_HDR < h->arena_free) {
        t_init(&t);
        t_s(&t, "CPU BIG ");
        size_text(&t, h->arena_largest);
        hrow(m, &t);
    }
    if (!(h->flags & DKRT_HEALTH_NO_DSP)) {
        t_init(&t);
        t_s(&t, "DSP FREE ");
        if (h->dsp_area_total != 0)
            size_text(&t, h->dsp_area_free);
        else
            t_s(&t, "--");
        hrow(m, &t);
        if (h->dsp_area_total != 0 && h->dsp_area_largest < h->dsp_area_free) {
            t_init(&t);
            t_s(&t, "DSP BIG ");
            size_text(&t, h->dsp_area_largest);
            hrow(m, &t);
        }
    }
    /* the card: the capacity the firmware's driver reports, in GB of 10^9 bytes with one decimal (sectors * 512 / 10^8 tenths, within 0.001 %) */
    t_init(&t);
    t_s(&t, "CARD ");
    if (h->card_sectors == 0xffffffffu || h->card_sectors == 0) {
        t_s(&t, "--");
    } else {
        uint32_t tenths = h->card_sectors / 195312u;
        t_num(&t, tenths / 10);
        t_c(&t, '.');
        t_num(&t, tenths % 10);
        t_s(&t, " GB");
    }
    hrow(m, &t);
    t_init(&t);
    t_s(&t, "STORE USED ");
    if (h->flags & DKRT_HEALTH_STORE)
        size_text_kb(&t, h->store_used);
    else
        t_s(&t, "--");
    hrow(m, &t);
    t_init(&t);
    t_s(&t, "STORE FREE ");
    if (h->flags & DKRT_HEALTH_STORE)
        size_text_kb(&t, h->store_free);
    else
        t_s(&t, "--");
    hrow(m, &t);
    if (!(h->flags & DKRT_HEALTH_MODULE)) {
        t_init(&t);
        t_s(&t, "NO HEALTH MODULE");
        hrow(m, &t);
    }
}

static void refresh_health(void)
{
    dkrt_health(&DK_HEALTH);
    dk_ui_menu_refresh(DK_HEALTH_MENU);
    DK_REDRAWS++;
}

/* The UI loop's per-pass call while the HEALTH page is registered: refresh about twice a second without a key press, and stop when
   the page was closed. */
void dk_poll(void)
{
    DK_POLLS++;
    if (!DK_HEALTH_MENU) {
        dkrt_poll_remove((uintptr_t)dk_poll);
        return;
    }
    if (dk_ui_menu_is_open(DK_HEALTH_MENU)) {
        g_seen = 1;
    } else if (g_seen) {
        dkrt_poll_remove((uintptr_t)dk_poll);
        g_seen = 0;
        return;
    } else {
        return;
    }
    uint32_t b = dkrt_blocks();
    if (b != 0) {
        if (g_last != 0 && b - g_last < H_BLOCKS)
            return;
        g_last = b;
    } else {
        if (++g_calls < 60)
            return;
        g_calls = 0;
    }
    refresh_health();
}

static void open_health(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    dkrt_health(&DK_HEALTH);
    DK_REDRAWS++;
    dk_ui_menu_open(DK_HEALTH_MENU);
    g_seen = 0;
    g_last = 0;
    g_calls = 0;
    dkrt_poll_add((uintptr_t)dk_poll);
}

/* ---- MODULES --------------------------------------------------------------------------------------------------------- */

static const char *kind_text(int i)
{
    const DkrtScan *e = &g_ent[i];
    if (!(e->flags & DKRT_SCAN_NEEDS))
        return "----";
    if (e->need_fx_slots)
        return "FX";
    if (e->need_machine_rows)
        return "MACH";
    return "TOOL";
}

/* A size in bytes as <1K, 259K or 1.2M. */
static void bytes_text(txt *t, uint32_t n)
{
    if (n >= 1024 * 1024) {
        uint32_t tenth = (n / 1024 * 10) / 1024;
        t_num(t, tenth / 10);
        t_c(t, '.');
        t_num(t, tenth % 10);
        t_c(t, 'M');
    } else if (n < 1024) {
        t_s(t, "<1K");
    } else {
        t_num(t, n / 1024);
        t_c(t, 'K');
    }
}

/* A module that the saved selection has and this boot did not load (safe mode, or the load failed): the check column cannot say "-", so
   the row text does (g_lead) and the check mark stays off. A module the user checked just now is a pending change and keeps its check mark. */
static int dashed(int i)
{
    return TICK[i] && g_saved[i] && !LOADED[i];
}

/* A module that is loaded although the saved selection does not have it: a failed or not attempted unload (it unloads at the next restart). */
static int unloads_at_restart(int i)
{
    return LOADED[i] && !g_saved[i] && !TICK[i];
}

/* The missing module of a dashed row (usb-bulk): the boot's load failed with E_IMPORT and the runtime kept the name; 0 when not. */
static const uint8_t *row_needs(int i)
{
    int k = g_sel_of[i];
    if (!dashed(i) || k < 0 || (uint32_t)k >= DKRT_SEL_MAX || (uint32_t)k >= g_boot.count)
        return 0;
    return g_boot.results[k] == DKRT_E_IMPORT && g_boot.needs[k][0] ? g_boot.needs[k] : 0;
}

/* Short names share aligned columns. A longer name moves the remaining columns right,
   so the stock list can scroll the complete selected row. */
static void module_text(int i, txt *t)
{
    t_init(t);
    if (g_lead)
        t_c(t, dashed(i) ? '-' : unloads_at_restart(i) ? '~' : PAD);
    const uint8_t *path = g_ent[i].path;
    if (i == g_busy_row) { /* the job of this row runs, or waits for the runtime's job slot */
        t_s(t, g_lq_state == LQ_START                  ? "WAITING"
               : g_lq_op[g_lq_pos] == DKRT_LIVE_UNLOAD ? "UNLOADING"
                                                       : "LOADING");
        t_c(t, ' ');
        name_of(path, stem_len(path), 63, 1, t);
        return;
    }
    name_of(path, stem_len(path), 63, 1, t);
    int kind_x = px_of(t->b) + 4;
    if (kind_x < (NAME_COLS + 1) * 4)
        kind_x = (NAME_COLS + 1) * 4;
    t_pad_px(t, kind_x);
    if (unloads_at_restart(i)) {
        t_s(t, "UNLOADS");
        return;
    }
    t_s(t, kind_text(i));
    txt sz;
    t_init(&sz);
    if (g_ent[i].flags & DKRT_SCAN_NEEDS)
        t_b(&sz, g_fit.rows[i].mem, DKRT_FIT_MEM);
    else
        bytes_text(&sz, g_ent[i].size);
    t_pad_px(t, kind_x + (4 + 1 + 5) * 4 - px_of(sz.b));
    t_s(t, sz.b);
}

static void module_row(dk_ui_menu *m, int i)
{
    txt t;
    module_text(i, &t);
    if (DK_LOAD_N < 40)
        keep(DK_LOAD[DK_LOAD_N++], t.b);
    dk_ui_menu_add_text(m, t.b);
}

static void fill_modules(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    DK_LOAD_N = 0;
    g_hdr = 0;
    g_empty_text[0] = 0;
    if (STORE != 1) {
        copy_text((uint8_t *)g_empty_text, sizeof g_empty_text,
                  STORE == 2   ? "NO STORE"
                  : STORE == 3 ? "STORE READ ERROR"
                               : "BAD STORE");
        keep(DK_LOAD[DK_LOAD_N++], g_empty_text);
        return;
    }
    if (NENT == 0) {
        copy_text((uint8_t *)g_empty_text, sizeof g_empty_text, "NO MODULES ON CARD");
        keep(DK_LOAD[DK_LOAD_N++], g_empty_text);
        return;
    }
    refit();
    g_lead = 0;
    for (uint32_t i = 0; i < NENT; i++)
        if ((g_saved[i] && !LOADED[i]) || unloads_at_restart((int)i))
            g_lead = 1;
    g_hdr = g_boot.safe != 0;
    if (g_hdr)
        dk_ui_menu_add_text(m, "SAFE MODE: NOT LOADED");
    for (uint32_t i = 0; i < NENT; i++)
        module_row(m, (int)i);
    for (uint32_t i = 0; i < MAX_ENT; i++)
        dk_ui_menu_check(m, (int)i + g_hdr, i < NENT && TICK[i] && !dashed((int)i));
    if (g_hdr)
        dk_ui_menu_check(m, 0, 0);
}

/* The check mark of a row after TICK changed. A saved row that is not loaded also changes its "-", which is row text. */
static void show_tick(int row)
{
    dk_ui_menu_check(g_modules, row + g_hdr, TICK[row] && !dashed(row));
    if (g_saved[row] && !LOADED[row])
        dk_ui_menu_refresh(g_modules);
}

static void refuse(int row, const char *what)
{
    txt title;
    t_init(&title);
    row_name(row, 10, &title);
    t_c(&title, ' ');
    t_s(&title, what);
    if (NOK[row]) {
        say(title.b, "BUILT FOR ANOTHER DEVICE OR RUNTIME", 0);
    } else {
        txt why;
        t_init(&why);
        t_b(&why, g_fit.rows[row].reason, DKRT_FIT_REASON);
        say(title.b, why.b, 0);
    }
}

/* Checks or unchecks a row. A module that does not fit next to the checked ones, or that this runtime or device cannot load, stays
   unchecked and a dialog says why. */
static void set_tick(int row, int on)
{
    if (g_lq_state != LQ_IDLE) /* a live apply runs: the list is read-only until it ends */
        return;
    if (g_selection_error != DKRT_OK) {
        fail_text("SELECTION READ ERROR", g_selection_error);
        return;
    }
    if (row < 0 || row >= (int)NENT)
        return;
    if (!on) {
        TICK[row] = 0;
        show_tick(row);
        return;
    }
    if (NOK[row]) {
        TICK[row] = 0;
        show_tick(row);
        refuse(row, "NOT COMPATIBLE");
        return;
    }
    TICK[row] = 1;
    refit();
    if (wont(row)) {
        TICK[row] = 0;
        show_tick(row);
        refuse(row, "DOES NOT FIT");
        return;
    }
    show_tick(row);
}

static void modules_check(void *ctx, int32_t row, int32_t on)
{
    (void)ctx;
    row -= g_hdr;
    if (row < 0) { /* the header is not a module */
        dk_ui_menu_check(g_modules, 0, 0);
        return;
    }
    if (row < (int)NENT &&
        dashed(row)) /* the check mark is off for it, so YES came as "on": it means off */
        on = 0;
    set_tick(row, on);
}

static void panel_load(void *ctx, int32_t row)
{
    (void)ctx;
    if (row >= 0 && row < (int)NENT)
        set_tick(row, !TICK[row]);
}

/* The loaded module of a store row that has a menu function: its dkrt_module index, or -1. */
static int settings_module(int row, DkrtModule *mod)
{
    if (row < 0 || row >= (int)NENT)
        return -1;
    for (uint32_t k = 0; k < MAX_SET && dkrt_module(k, mod) == DKRT_OK; k++)
        if (same_text(g_ent[row].path, mod->path, 63))
            return mod->menu && mod->state == DKRT_MOD_LOADED ? (int)k : -1;
    return -1;
}

/* The fill of a module's settings menu: the module adds the rows. ctx is the cache slot; the module is found by the path the slot holds. */
static void fill_settings(dk_ui_menu *m, void *ctx)
{
    static DkrtModule mod;
    uint32_t slot = (uint32_t)(uintptr_t)ctx;
    /* The rows a module adds hold pointers into its code (callbacks) and data. They exist only while the page is open: g_set_open is set at the open and cleared
       when the page is left (settings_release), and a fill after that, or one for a module that is gone, adds a text row only. So the page keeps no pointer into a
       module that can be unloaded. */
    if (g_set_open[slot])
        for (uint32_t k = 0; dkrt_module(k, &mod) == DKRT_OK; k++)
            if (same_text(mod.path, g_set_path[slot], 63)) {
                if (mod.menu) {
                    DK_CALL_MENU(mod.menu, mod.ctx, m);
                    return;
                }
                break;
            }
    dk_ui_menu_add_text(m, g_set_open[slot] ? "MODULE NOT LOADED" : "CLOSED");
}

/* The page of a slot that was left: fill it again without the module, which drops the module's rows. All of them at once. */
static void settings_release(void)
{
    for (int k = 0; k < MAX_SET; k++)
        if (g_set_open[k] && (!g_set[k] || !dk_ui_menu_is_open(g_set[k]))) {
            g_set_open[k] = 0;
            if (g_set[k])
                dk_ui_menu_refresh(
                    g_set
                        [k]); /* the rows of the module are gone before the module can be unloaded */
            if (g_set_hold[k]) {
                dkrt_menu_release(g_set_hold[k], 0);
                g_set_hold[k] = 0;
            }
        }
}

/* The UI loop's per-pass call while a settings page is open: release the pages the user left. */
static void settings_poll(void)
{
    settings_release();
    for (int k = 0; k < MAX_SET; k++)
        if (g_set_open[k])
            return;
    dkrt_poll_remove((uintptr_t)settings_poll);
}

/* The cache slot of the module at store path p: the slot that holds it, else a free one (taken for it), else -1. */
static int settings_slot(const uint8_t *p)
{
    for (int s = 0; s < MAX_SET; s++)
        if (g_set_path[s][0] && same_text(g_set_path[s], p, 63))
            return s;
    for (int s = 0; s < MAX_SET; s++)
        if (!g_set_path[s][0]) {
            for (int c = 0; c < 63 && p[c]; c++)
                g_set_path[s][c] = p[c];
            return s;
        }
    return -1;
}

static void panel_settings(void *ctx, int32_t row)
{
    (void)ctx;
    if (g_lq_state != LQ_IDLE) /* a live apply runs */
        return;
    static DkrtModule mod;
    if (settings_module(row, &mod) < 0)
        return;
    int k = settings_slot(mod.path);
    if (k < 0)
        return;
    if (!g_set[k]) {
        /* the title is the module's name without its author (digikit/remote: REMOTE); the wide title bar holds 28 characters; the stock bar cuts a longer one at the start with '..' */
        int from = 0;
        for (int c = 0; c < 32 && mod.name[c]; c++)
            if (mod.name[c] == '/')
                from = c + 1;
        copy_text((uint8_t *)g_set_title[k], NAME_ROW, "");
        for (int c = 0; c < NAME_ROW - 1 && mod.name[from + c]; c++)
            g_set_title[k][c] = upper((char)mod.name[from + c]);
        g_set_desc[k].name = "DigikitModuleMenu";
        g_set_desc[k].title = g_set_title[k];
        g_set_desc[k].flags = DK_UI_WIDE;
        g_set_desc[k].fill = fill_settings;
        g_set_desc[k].arg = (void *)(uintptr_t)k;
        g_set[k] = dk_ui_menu_new(g_ui, &g_set_desc[k]);
        if (!g_set[k])
            g_set_path[k][0] = 0; /* the slot is free again */
    }
    if (g_set[k]) {
        if (!g_set_hold[k]) {
            g_set_hold[k] =
                (uintptr_t)mod
                    .menu; /* before the module adds its rows: the module stays loaded while the page is open */
            dkrt_menu_built(g_set_hold[k], 0);
        }
        g_set_open[k] = 1;
        dkrt_poll_add((uintptr_t)settings_poll);
        dk_ui_menu_open(g_set[k]);
    }
}

static void panel_info(void *ctx, int32_t row)
{
    (void)ctx;
    if (g_lq_state != LQ_IDLE) /* a live apply runs */
        return;
    g_info_row = row;
    dk_ui_menu_open(g_info);
}

static void prow(dk_ui_menu *p, const char *text, dk_ui_action_fn *fn, int32_t row)
{
    if (DK_PANEL_N < 4)
        keep(DK_PANEL[DK_PANEL_N++], text);
    dk_ui_menu_add_action(p, text, fn, 0, row);
}

static void modules_panel(dk_ui_menu *p, int32_t row, void *ctx)
{
    (void)ctx;
    DK_PANEL_N = 0;
    row -= g_hdr;
    if (row < 0 || row >= (int)NENT ||
        g_lq_state != LQ_IDLE) /* no side panel while a live apply runs */
        return;
    prow(p, TICK[row] ? "UNLOAD" : "LOAD", panel_load, row);
    DkrtModule mod;
    if (settings_module(row, &mod) >= 0)
        prow(p, "SETTINGS...", panel_settings, row);
    prow(p, "INFO", panel_info, row);
    /* the detail of a dashed row, last so that the cursor starts on LOAD: why it is not loaded (a text row, no action) */
    const uint8_t *needs = row_needs(row);
    if (needs) {
        txt t;
        t_init(&t);
        t_s(&t, "NEEDS ");
        t_needs(&t, needs);
        if (DK_PANEL_N < 4)
            keep(DK_PANEL[DK_PANEL_N++], t.b);
        dk_ui_menu_add_text(p, t.b);
    }
}

/* ---- INFO ------------------------------------------------------------------------------------------------------------ */

static void irow(dk_ui_menu *m, const txt *t)
{
    dk_ui_menu_add_text(m, t->b);
}

static void fill_info(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    int i = g_info_row;
    if (i < 0 || i >= (int)NENT) {
        dk_ui_menu_add_text(m, "NO MODULE");
        return;
    }
    const DkrtScan *e = &g_ent[i];
    txt t;
    t_init(&t);
    row_name(i, 23, &t);
    irow(m, &t);
    /* the version is in the file's header; the runtime reads it */
    t_init(&t);
    t_s(&t, "VERSION ");
    DkrtModuleHeader hdr;
    if (dkrt_module_header(e->first, &hdr) == DKRT_OK) {
        t_num(&t, hdr.version >> 16);
        t_c(&t, '.');
        t_num(&t, hdr.version >> 8 & 255);
        t_c(&t, '.');
        t_num(&t, hdr.version & 255);
    } else {
        t_s(&t, "--");
    }
    irow(m, &t);
    t_init(&t);
    t_s(&t, "SIZE ");
    if (e->flags & DKRT_SCAN_NEEDS)
        t_b(&t, g_fit.rows[i].mem, DKRT_FIT_MEM);
    else
        bytes_text(&t, e->size);
    irow(m, &t);
    /* the module alone: its share of the DSP and the CPU memory */
    static uint8_t only[MAX_ENT];
    for (uint32_t k = 0; k < MAX_ENT; k++)
        only[k] = (int)k == i;
    dkrt_fit(g_ent, NENT, only, &g_fit);
    t_init(&t);
    const uint8_t *line = g_fit.line;
    int cut = 0;
    while (line[cut] && !(line[cut] == ' ' && line[cut + 1] == 'C' && line[cut + 2] == 'P'))
        cut++;
    if (line[0]) {
        t_b(&t, line, cut);
        irow(m, &t);
        t_init(&t);
        t_b(&t, line + (line[cut] ? cut + 1 : cut), DKRT_FIT_LINE);
        irow(m, &t);
    } else {
        t_s(&t, "DSP --");
        irow(m, &t);
        t_init(&t);
        t_s(&t, "CPU --");
        irow(m, &t);
    }
    /* does it fit next to the checked ones */
    for (uint32_t k = 0; k < MAX_ENT; k++)
        only[k] = TICK[k] || (int)k == i;
    dkrt_fit(g_ent, NENT, only, &g_fit);
    t_init(&t);
    if (NOK[i]) {
        t_s(&t, "FIT: NOT COMPATIBLE");
    } else if (wont(i)) {
        t_s(&t, "NO FIT: ");
        t_b(&t, g_fit.rows[i].reason, DKRT_FIT_REASON);
    } else {
        t_s(&t, "FITS");
    }
    irow(m, &t);
    t_init(&t);
    t_s(&t, "STATUS ");
    t_s(&t, e->flags & DKRT_SCAN_LOADING  ? "LOADING"
            : e->flags & DKRT_SCAN_LOADED ? "LOADED"
                                          : "NOT LOADED");
    irow(m, &t);
    if (row_needs(i)) {
        t_init(&t);
        t_s(&t, "NEEDS ");
        t_needs(&t, row_needs(i));
        irow(m, &t);
    }
    if ((TICK[i] != 0) != (LOADED[i] != 0)) {
        t_init(&t);
        t_s(&t, TICK[i] ? "LOADS AT RESTART" : "UNLOADS AT RESTART");
        irow(m, &t);
    }
    /* the next fill of the list shows the verdicts of the checked set again */
    refit();
}

/* ---- leaving MODULES: APPLY n CHANGES? ------------------------------------------------------------------------------- */

static void live_poll(void);

static void restart_answer(void *ctx, int32_t yes)
{
    (void)ctx;
    if (!yes)
        return;
    int32_t r = dkrt_restart();
    DK_APPLY_RESTART = r;
    dk_ui_menu_refresh(
        g_modules); /* the saved selection against what is loaded: the "-" and "UNLOADS" marks */
    fail_text("NO RESTART", r);
}

/* After the "<NAME> NOT APPLIED" dialog: the saved selection loads at a restart, so offer it. */
static void not_applied_done(void *ctx, int32_t yes)
{
    (void)ctx;
    (void)yes;
    say("RESTART TO APPLY?", "THE CHANGE IS SAVED", restart_answer);
}

static int row_by_path(const uint8_t *p)
{
    for (uint32_t i = 0; i < NENT; i++)
        if (same_text(g_ent[i].path, p, 63))
            return (int)i;
    return -1;
}

/* Scans again after a job: the runtime's counters and which rows are loaded; the list shows it. */
static void live_rescan(void)
{
    read_boot();
    scan_store();
    dk_ui_menu_refresh(g_modules);
}

static void live_end(void)
{
    g_lq_state = LQ_IDLE;
    g_busy_row = -1;
    dkrt_poll_remove((uintptr_t)live_poll);
}

/* The batch ended without a job's success: the dialog with the status text, then the restart question (the selection is saved). */
static void live_stop(const uint8_t *path, int32_t code)
{
    DK_LIVE_RESULT = code;
    live_end();
    live_rescan();
    txt title, why;
    t_init(&title);
    if (path) {
        path_name(path, 10, &title);
        t_s(&title, " NOT APPLIED");
    } else {
        t_s(&title, "NOT APPLIED");
    }
    t_init(&why);
    status_text(&why, code);
    say(title.b, why.b, not_applied_done);
}

/* The UI loop's per-pass call while a batch runs: queue the next job, then wait for it. One job at a time (the runtime has one). */
static void live_poll(void)
{
    if (g_lq_state == LQ_IDLE) {
        dkrt_poll_remove((uintptr_t)live_poll);
        return;
    }
    const uint8_t *path = g_lq_path[g_lq_pos];
    if (g_lq_state == LQ_START) {
        int32_t r = dkrt_live(g_lq_op[g_lq_pos], path);
        if (r == DKRT_E_BUSY && ++g_lq_wait < LQ_BUSY_PASSES) {
            if (g_lq_wait == 1) { /* the row reads WAITING */
                g_busy_row = row_by_path(path);
                dk_ui_menu_refresh(g_modules);
            }
            return;
        }
        if (r != DKRT_OK) {
            live_stop(path, r);
            return;
        }
        DK_LIVE_JOBS++;
        g_lq_state = LQ_RUN;
        g_busy_row = row_by_path(path);
        dk_ui_menu_refresh(g_modules); /* the row reads LOADING */
        return;
    }
    DkrtLive st;
    dkrt_live_state(&st);
    if (st.phase != 2)
        return;
    if (st.result != DKRT_OK) {
        live_stop(path, st.result);
        return;
    }
    g_busy_row = -1;
    g_lq_pos++;
    g_lq_wait = 0;
    live_rescan();
    if (g_lq_pos >= g_lq_n) {
        live_end();
        if (dk_ui_menu_is_open(g_modules))
            dk_ui_menu_close(g_modules);
        return;
    }
    g_lq_state = LQ_START;
}

static void live_add(uint32_t i, uint32_t op)
{
    for (int c = 0; c < 64; c++)
        g_lq_path[g_lq_n][c] = g_ent[i].path[c];
    g_lq_op[g_lq_n++] = (uint8_t)op;
}

/* The rows the user changed since the saved state, as jobs: the unloads, then the loads. A row the boot did not load that stays checked is no change. */
static void live_plan(void)
{
    g_lq_n = 0;
    g_lq_pos = 0;
    g_lq_wait = 0;
    for (uint32_t i = 0; i < NENT; i++)
        if (TICK[i] != g_saved[i] && !TICK[i] && LOADED[i])
            live_add(i, DKRT_LIVE_UNLOAD);
    for (uint32_t i = 0; i < NENT; i++)
        if (TICK[i] != g_saved[i] && TICK[i] && !LOADED[i])
            live_add(i, DKRT_LIVE_LOAD);
}

/* Saves the selection. Leaving safe mode restarts (dkrt_restart returns only when it refuses); every other
   change runs live: the unloads, then the loads, polled from the UI loop. */
static void apply(void)
{
    if (g_selection_error != DKRT_OK) {
        fail_text("NOT SAVED", g_selection_error);
        return;
    }
    DK_APPLIES++;
    static DkrtSelection sel;
    uint32_t n = 0;
    sel.flags = 0;
    for (uint32_t i = 0; i < DKRT_SEL_MAX; i++)
        for (uint32_t c = 0; c < DKRT_SEL_PATH; c++)
            sel.paths[i][c] = 0;
    for (uint32_t i = 0; i < NENT; i++) {
        if (TICK[i] && n < DKRT_SEL_MAX) {
            const uint8_t *p = g_ent[i].path;
            int l = path_len(p, 63);
            if (l < DKRT_SEL_PATH) {
                for (int c = 0; c < l; c++)
                    sel.paths[n][c] = p[c];
                n++;
            }
        }
    }
    sel.count = n;
    int32_t r = dkrt_selection_set(&sel);
    DK_APPLY_SET = r;
    if (r != DKRT_OK) {
        fail_text("NOT SAVED", r);
        return;
    }
    int restart = g_boot.safe != 0;
    live_plan();
    for (uint32_t i = 0; i < MAX_ENT; i++)
        g_saved[i] = TICK[i];
    g_saved_extra = 0;
    if (restart) {
        r = dkrt_restart();
        DK_APPLY_RESTART = r;
        dk_ui_menu_refresh(
            g_modules); /* a refused restart: the list shows what the saved selection still has to change */
        fail_text("NO RESTART", r);
        return;
    }
    if (g_lq_n ==
        0) { /* nothing to load or unload (a dashed row unchecked): the selection is saved */
        dk_ui_menu_close(g_modules);
        return;
    }
    settings_release(); /* no page keeps rows of a module the batch may unload */
    g_lq_state = LQ_START;
    r = dkrt_poll_add((uintptr_t)live_poll);
    if (r != DKRT_OK) { /* the batch cannot run: the selection is saved, so a restart applies it */
        g_lq_state = LQ_IDLE;
        live_stop(0, r);
    }
}

/* NO on the confirm drops the pending changes and closes the list. */
static void discard(void)
{
    for (uint32_t i = 0; i < MAX_ENT; i++)
        TICK[i] = g_saved[i];
    dk_ui_menu_close(g_modules);
}

static void apply_answer(void *ctx, int32_t yes)
{
    (void)ctx;
    if (yes)
        apply();
    else
        discard();
}

static int32_t modules_back(void *ctx)
{
    (void)ctx;
    if (g_lq_state != LQ_IDLE) /* a live apply runs */
        return 1;
    uint32_t n = changes();
    DK_PENDING = n;
    if (!restart_pending())
        return 0;
    txt t;
    t_init(&t);
    if (g_boot.safe == 0) {
        t_s(&t, "APPLY ");
        t_num(&t, n);
        t_s(&t, n == 1 ? " CHANGE?" : " CHANGES?");
        say(t.b, "NO RESTART", apply_answer);
        return 1;
    }
    if (n) {
        t_num(&t, n);
        t_s(&t, n == 1 ? " CHANGE" : " CHANGES");
    } else {
        t_s(&t, "LEAVES SAFE MODE");
    }
    say("APPLY & RESTART?", t.b, apply_answer);
    return 1;
}

static void open_modules(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    read_boot();
    scan_store();
    dk_ui_menu_select(g_modules,
                      STORE == 1 && NENT > 0 &&
                          g_boot.safe != 0); /* the first module, below the safe-mode header */
    dk_ui_menu_open(g_modules);
}

/* ---- the SETTINGS row ------------------------------------------------------------------------------------------------ */

static const dk_ui_menu_desc main_desc = {
    .name = "DigikitMenu", .title = "DIGIKIT", .fill = fill_main};
static const dk_ui_menu_desc about_desc = {
    .name = "DigikitAboutMenu", .title = "ABOUT", .flags = DK_UI_TEXT_PAGE, .fill = fill_about};
static const dk_ui_menu_desc health_desc = {
    .name = "DigikitHealthMenu", .title = "HEALTH", .flags = DK_UI_TEXT_PAGE, .fill = fill_health};
static const dk_ui_menu_desc mem_desc = {
    .name = "DigikitMemoryMenu", .title = "MEMORY", .flags = DK_UI_TEXT_PAGE, .fill = fill_mem};
static const dk_ui_menu_desc info_desc = {
    .name = "DigikitInfoMenu", .title = "INFO", .flags = DK_UI_TEXT_PAGE, .fill = fill_info};
static const dk_ui_menu_desc modules_desc = {.name = "DigikitModulesMenu",
                                             .title = "MODULES",
                                             .flags = DK_UI_LIST,
                                             .fill = fill_modules,
                                             .empty = g_empty_text,
                                             .panel = modules_panel,
                                             .check = modules_check,
                                             .back = modules_back};

static void open_main(void *ctx, int32_t arg)
{
    (void)ctx;
    (void)arg;
    read_boot();
    if (!g_main) {
        g_ui = dk_ui_core_handle();
        g_about = dk_ui_menu_new(g_ui, &about_desc);
        DK_HEALTH_MENU = dk_ui_menu_new(g_ui, &health_desc);
        g_mem = dk_ui_menu_new(g_ui, &mem_desc);
        g_info = dk_ui_menu_new(g_ui, &info_desc);
        g_modules = dk_ui_menu_new(g_ui, &modules_desc);
        g_main = dk_ui_menu_new(g_ui, &main_desc);
    }
    if (!g_main)
        return;
    dk_ui_menu_open(g_main);
    if (g_boot.safe != 0 && !g_safe_shown) {
        g_safe_shown = 1;
        safe_notice(0, 0);
    }
}

/* Called by core/ui from its hook at the end of the SETTINGS rebuild, with the settings menu: adds DIGIKIT after SYSTEM. */
void dk_app_settings_rows(uint8_t *settings_menu)
{
    dk_ui_stock_row(settings_menu, "DIGIKIT", open_main, 0);
}

/* For the test that calls the module in place: the number of .dkm entries found by the last scan. */
uint32_t dk_entry_count(void)
{
    return NENT;
}
/* x */
