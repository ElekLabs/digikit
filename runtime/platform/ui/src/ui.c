/* core/ui: the device adapter behind <dk/platform/ui.h> (docs/plans/sdk-api.md phase 3, docs/findings/71-ui-components-1-17.md).
 *
 * The same source runs on every device that has the digikit menu: the device's facts come from the device file and every firmware call is a generated anchor (dk_fw_*.h).
 *
 * Views. A plain menu or text page is a stock VerticalMenuView, a DK_UI_LIST is a stock MenuWithPopoutSideBarsView (a list with a
 * title bar, check marks and a right side panel). Each is built once, in the firmware heap, and kept for the session through a
 * shared_ptr<View> this module owns (dk_make_sp), so the firmware never frees it. The view stack only holds copies.
 *
 * Rows are stock MenuItems. Their std::functions keep two words in the function's own storage (the menu and the row number), so
 * a copy copies two words, a destroy does nothing and nothing is allocated per row. The row data (callback, context, value
 * pointer, limits) is in an array owned by the menu; a refill rewrites it.
 *
 * Own vtables. The View sub-object of a view reaches its paint and key handler through the View vtable, whose entries are
 * thunks that branch to the real function. A list and a text page get a copy of that vtable (module RAM, written once) with
 * slot 4 (paint) and, for a list, slot 2 (key) replaced; the replacement calls the stock thunk for everything it does not handle.
 *
 * Dialogs. ConfirmWindow and MultiChoiceWindow are make_shared blocks built by hand (0x10 control block, then the object), pushed
 * on the Brain's view manager. The firmware frees them in its sweep. One dialog at a time: g_dialog (dialog_slot.h) holds the serial of the open
 * one, who asked for it and its answer function; the answer callback frees the slot, and so does the detach callback of the window (a choice cancelled
 * with NO has no answer). The window keeps no pointer into the asker's code: when the asker is unloaded the slot drops its answer function.
 *
 * Module RAM is written only by this code, never by the firmware (the menu proof checks stray writes). */
#include <stdint.h>
#include <stddef.h>
#include <dk/core/status.h>
#include <dk/gen/imports.h>
#include <dk/platform/ui.h>
#include "dk_ui_core.h"
#include <dk/platform/ui_text.h>
#include "tick.h"
#include "settings_rows.h"
#include "dialog_slot.h"
#include "view_pool.h"
#include "canvas_map.h"
#include "text_input.h"
#include "key_defer.h"
#include "dkrt.h"
#include "dk_fw_mem.h"
#include "dk_fw_string.h"
#include "dk_fw_view.h"
#include "dk_fw_menu.h"
#include "dk_fw_window.h"
#include "dk_fw_text.h"
#include "dk_fw_draw.h"
#include "dk_fw_input.h"

/* The control-block vtables of the two make_shared window types (data anchors; the address is the vtable). */
extern const uint8_t fw_confirm_cb_vt __asm__("cf.window.site.confirm_cb_vptr_lit");
extern const uint8_t fw_choice_cb_vt __asm__("cf.window.site.multichoice_cb_vptr_lit");
/* The control-block vtable of the make_shared<NameView> block. */
extern const uint8_t fw_name_cb_vt __asm__("cf.window.site.name_cb_vptr_lit");

/* assembly shim */
void dk_make_sp(shared_ptr *out, uint8_t *parent, uint8_t *view);

#define VIEW_AT 0x38     /* the View sub-object of a Menu view */
#define CMP_FONT_AT 0x94 /* CompactVerticalMenuView: the font object itself */
#define POP_PANEL_AT 0x380
#define POP_EXT_AT 0x548 /* our word behind a popout view: the dk_ui_menu */
#define VT_WORDS                                                                                   \
    19 /* View vtable: offset_to_top, typeinfo, 17 slots (the rule cf.view.site.view_vptr_lit: 17 slots start at the vptr, the 8-byte header before it) */
#define VT_PAINT 4
#define VT_KEY 2
#define VT_DETACH_ADD 16
#define VT_CLOSE 10
/* The key ids of the firmware's key events (the id at +0xc of the event): the device's `[input.keys]` (YES, LEVEL_PUSH as the second YES, NO, RIGHT) and `[input.key_flags]`, in the per-device object src/keys/<dev>.s. */
extern const uint32_t dk_ui_view_keys[6];
#define KEY_YES (dk_ui_view_keys[0])
/* KEY_YES2 is the id of LEVEL_PUSH, a second key that the stock handler treats like YES. */
#define KEY_YES2 (dk_ui_view_keys[1])
#define KEY_NO (dk_ui_view_keys[2])
#define KEY_RIGHT (dk_ui_view_keys[3])
/* The flags word of a key event (+0x10): the bit set on a press (the move keys act on it) and the bit set on a release (YES and NO act on it), as the firmware's own
   handler reads them. Device data: `[input.key_flags]`. */
#define KEYFL_PRESS (1u << dk_ui_view_keys[4])
#define KEYFL_RELEASE (1u << dk_ui_view_keys[5])
#define MAX_CHOICE 32
#define LIST_ROWS 8
#define LIST_COLS 20 /* characters of a list row that the label of a toggle or value row fills */

enum { K_MENU, K_TEXT, K_LIST, K_PANEL };
enum { R_ACTION, R_SUBMENU, R_TEXT, R_TOGGLE, R_VALUE };

typedef struct row {
    uint8_t kind;
    dk_ui_action_fn *fn;
    void *ctx;
    int arg;
    int *value;
    int min, max;
    dk_ui_menu *sub;
} row;

struct dk_ui_menu {
    const dk_ui_menu_desc *d;
    uint8_t *obj;   /* the Menu object; the View sub-object is at +0x38 */
    shared_ptr *sp; /* kept for the session; in the firmware heap */
    uint8_t *font;
    uint8_t kind;
    uint8_t filling;
    dk_ui_menu *owner; /* a panel: its list */
    dk_ui_menu *panel; /* a list: its side panel */
    row *rows;
    int n, cap;
};

/* std::function<...> as the firmware lays it out: 8 bytes of storage, manager, invoker. */
typedef struct fn {
    uint32_t w0, w1, mgr, inv;
} fn;

typedef uint32_t (*mgr_fn)(uint32_t *dest, const uint32_t *src, uint32_t op);

static const uint32_t typeinfo_dummy = 0;

static uint32_t fn_manager(uint32_t *dest, const uint32_t *src, uint32_t op)
{
    switch (op) {
    case 0:
        *dest = (uint32_t)&typeinfo_dummy;
        break;
    case 1:
        *dest = (uint32_t)src;
        break;
    case 2:
        dest[0] = src[0];
        dest[1] = src[1];
        break;
    default:
        break;
    }
    return 0;
}

static fn make_fn(uint32_t w0, uint32_t w1, void *inv)
{
    fn f = {w0, w1, (uint32_t)fn_manager, (uint32_t)inv};
    return f;
}

/* ---- small helpers ---------------------------------------------------------------------------------------------------- */

static uint32_t *vtable(uint8_t *p)
{
    return *(uint32_t **)p;
}

static void zero(void *p, uint32_t n)
{
    volatile uint8_t *b = (volatile uint8_t *)p;
    while (n--)
        *b++ = 0;
}

static uint8_t *fw_new(uint32_t n)
{
    uint8_t *p = dk_fw_mem_new(n);
    if (p)
        zero(p, n);
    return p;
}

/* Copies at most max-1 bytes of s into buf and terminates it. */
static void cut(char *buf, uint32_t max, const char *s)
{
    uint32_t i = 0;
    if (s)
        while (s[i] && i + 1 < max) {
            buf[i] = s[i];
            i++;
        }
    buf[i] = 0;
}

static uint32_t read_u32(const uint8_t *p)
{
    return *(const uint32_t *)p;
}

static void invalidate(dk_ui_menu *m)
{
    dk_fw_view_invalidate(m->obj + VIEW_AT);
}

static uint8_t *view_manager(void)
{
    return dk_fw_view_brain_view_manager(dk_fw_view_brain_instance());
}

/* The font of a VerticalMenuView: the operand of the `move.l #font,d0` in its constructor (the field has another offset on some devices). */
static uint8_t *vmv_font(void)
{
    const uint16_t *r = (const uint16_t *)&dk_fw_menu_vertical_menu_view_ctor_font_ref;
    __asm__("" : "+r"(r)); /* the compiler must not see the 1-byte object */
    return (uint8_t *)(((uint32_t)r[1] << 16) | r[2]);
}

/* The stock icon the builder always wants: the operand of the `pea` of the SETTINGS > SYSTEM item. */
static const uint8_t *stock_icon(void)
{
    const uint16_t *r = (const uint16_t *)&dk_fw_menu_settings_list_system_icon_ref;
    __asm__("" : "+r"(r)); /* the compiler must not see the 1-byte object */
    return (const uint8_t *)(((uint32_t)r[1] << 16) | r[2]);
}

/* A zeroed heap block the size of a VerticalMenuView's tail: the parent dk_make_sp clears a shared_ptr in (at +0x90). */
static uint8_t *g_parent;

/* ---- rows ------------------------------------------------------------------------------------------------------------- */

static void run_row(dk_ui_menu *m, int i);
static void draw_row(dk_ui_menu *m, int i, uint8_t *canvas, int x, int y);
static void adjust_row(dk_ui_menu *m, int i, int delta);

static void inv_act(const uint32_t *any)
{
    run_row((dk_ui_menu *)any[0], (int)any[1]);
}

static void inv_draw(const uint32_t *any, uint8_t *item, uint8_t *canvas, int x, int y)
{
    (void)item;
    draw_row((dk_ui_menu *)any[0], (int)any[1], canvas, x, y);
}

static void inv_adjust(const uint32_t *any, uint8_t *item, int delta)
{
    (void)item;
    adjust_row((dk_ui_menu *)any[0], (int)any[1], delta);
}

/* Field by field: a struct assignment would call memcpy, which this module does not link. */
static void rowcpy(row *d, const row *s)
{
    d->kind = s->kind;
    d->fn = s->fn;
    d->ctx = s->ctx;
    d->arg = s->arg;
    d->value = s->value;
    d->min = s->min;
    d->max = s->max;
    d->sub = s->sub;
}

static row *add_row(dk_ui_menu *m)
{
    if (m->n == m->cap) {
        int cap = m->cap ? m->cap * 2 : LIST_ROWS;
        row *r = (row *)fw_new((uint32_t)cap * sizeof(row));
        if (!r)
            return 0;
        for (int i = 0; i < m->n; i++)
            rowcpy(&r[i], &m->rows[i]);
        if (m->rows)
            dk_fw_mem_free((uint8_t *)m->rows);
        m->rows = r;
        m->cap = cap;
    }
    row *r = &m->rows[m->n];
    r->kind = 0;
    r->fn = 0;
    r->ctx = 0;
    r->arg = 0;
    r->value = 0;
    r->min = r->max = 0;
    r->sub = 0;
    return r;
}

/* One MenuItem with a text label and an action. `adj` is an empty or an encoder function, `draw` 0 (no value drawn) or the function
   that draws the value. `keep_icon`: keep the stock icon the builder gives every item (a row among the stock SETTINGS rows). */
static uint8_t *make_item(const char *text, const fn *act, const fn *adj, const fn *draw,
                          int keep_icon, uint32_t label_size)
{
    char label[DK_UI_LIST_TEXT];
    cut(label, label_size, text);
    uint8_t *item = fw_new(sizeof(MenuItem));
    if (!item)
        return 0;
    dk_fw_menu_item_builder(item, label, (const std_function *)act, stock_icon(),
                            (const std_function *)adj);
    if (keep_icon)
        return item;
    /* the builder gave the item the stock icon's draw function; drop it (its manager frees the copied bitmap) */
    fn *d = (fn *)(item + offsetof(MenuItem, draw));
    if (d->mgr)
        ((mgr_fn)d->mgr)(&d->w0, &d->w0, 3);
    if (draw) {
        d->w0 = draw->w0;
        d->w1 = draw->w1;
        d->mgr = draw->mgr;
        d->inv = draw->inv;
    } else {
        d->w0 = d->w1 = d->mgr = d->inv = 0;
    }
    return item;
}

/* A list calls the draw function of an item with an origin that is not the row's (a value drawn there lands on the title bar). A toggle or
   value row of a list shows its value in its label instead: the text, then ON, OFF or the number (three digits) at the right end of
   LIST_COLS columns. A change fills the list again (value_changed), which writes the label again. */
static void value_label(char *out, const char *text, const row *r)
{
    char val[4];
    int v = r->value ? *r->value : 0, vn = 3, n = 0;
    if (r->kind == R_TOGGLE) {
        val[0] = 'O';
        val[1] = v ? 'N' : 'F';
        val[2] = 'F';
        vn = v ? 2 : 3;
    } else {
        val[0] = v < 0 ? '-' : (char)('0' + (v / 100) % 10);
        if (v < 0)
            v = -v;
        val[1] = (char)('0' + (v / 10) % 10);
        val[2] = (char)('0' + v % 10);
    }
    while (text[n] && n < LIST_COLS - vn - 1) {
        out[n] = text[n];
        n++;
    }
    while (n < LIST_COLS - vn)
        out[n++] = ' ';
    for (int k = 0; k < vn; k++)
        out[n++] = val[k];
    out[n] = 0;
}

/* A toggle or value changed: a list writes its labels again (the selection stays), a menu draws the value. */
static void refill(dk_ui_menu *m, int top);
static void value_changed(dk_ui_menu *m)
{
    if (m->kind == K_LIST)
        refill(m, 0);
    else
        invalidate(m);
}

static void add_item(dk_ui_menu *m, const char *text, int i, int with_value)
{
    char label[DK_UI_ROW];
    int in_list = m->kind == K_LIST && with_value &&
                  (m->rows[i].kind == R_TOGGLE || m->rows[i].kind == R_VALUE);
    if (in_list) {
        value_label(label, text, &m->rows[i]);
        text = label;
    }
    fn act = make_fn((uint32_t)m, (uint32_t)i, inv_act);
    fn adj = make_fn(0, 0, 0);
    adj.mgr = 0;
    if (with_value && m->rows[i].kind == R_VALUE)
        adj = make_fn((uint32_t)m, (uint32_t)i, inv_adjust);
    fn draw = make_fn((uint32_t)m, (uint32_t)i, inv_draw);
    int list_text = m->kind == K_LIST && i < m->n && m->rows[i].kind == R_TEXT;
    uint8_t *item = make_item(text, &act, &adj, with_value && !in_list ? &draw : 0, 0,
                              list_text ? DK_UI_LIST_TEXT : DK_UI_ROW);
    if (item)
        dk_fw_menu_add_item(m->obj, item);
}

/* For other core modules: a row among the rows of a stock menu (SETTINGS), with its icon. fn(ctx, 0) runs on YES. */
static void inv_stock(const uint32_t *any)
{
    ((dk_ui_action_fn *)any[0])((void *)any[1], 0);
}

void dk_ui_stock_row(uint8_t *stock_menu, const char *text, dk_ui_action_fn *fn_, void *ctx)
{
    fn act = make_fn((uint32_t)fn_, (uint32_t)ctx, inv_stock);
    fn adj = make_fn(0, 0, 0);
    adj.mgr = 0;
    uint8_t *item = make_item(text, &act, &adj, 0, 1, DK_UI_ROW);
    if (item)
        dk_fw_menu_add_item(stock_menu, item);
}

static void add(dk_ui_menu *m, const char *text, uint8_t kind, dk_ui_action_fn *fn_, void *ctx,
                int arg, int with_value)
{
    if (!m)
        return;
    row *r = add_row(m);
    if (!r)
        return;
    r->kind = kind;
    r->fn = fn_;
    r->ctx = ctx;
    r->arg = arg;
    m->n++;
    add_item(m, text, m->n - 1, with_value);
    invalidate(m);
}

static void list_focus_main(dk_ui_menu *m)
{
    if (m->kind == K_PANEL && m->owner)
        dk_fw_menu_popout_set_active(m->owner->obj, m->owner->obj);
}

static int ui_open(dk_ui_menu *m);

static void run_row(dk_ui_menu *m, int i)
{
    if (!m || i < 0 || i >= m->n)
        return;
    row r;
    rowcpy(&r, &m->rows[i]); /* a callback may refill the menu */
    list_focus_main(m);
    switch (r.kind) {
    case R_ACTION:
        if (r.fn)
            r.fn(r.ctx, r.arg);
        break;
    case R_SUBMENU:
        ui_open(r.sub);
        break;
    case R_TOGGLE:
        if (r.value) {
            int v = *r.value == 0;
            *r.value = v;
            value_changed(m);
            if (r.fn)
                r.fn(r.ctx, v);
        }
        break;
    default:
        break;
    }
}

static void adjust_row(dk_ui_menu *m, int i, int delta)
{
    if (!m || i < 0 || i >= m->n || m->rows[i].kind != R_VALUE || !m->rows[i].value)
        return;
    row r;
    rowcpy(&r, &m->rows[i]);
    int v = *r.value + delta;
    if (v > r.max)
        v = r.max;
    if (v < r.min)
        v = r.min;
    if (v == *r.value)
        return;
    *r.value = v;
    value_changed(m);
    if (r.fn)
        r.fn(r.ctx, v);
}

static void draw_row(dk_ui_menu *m, int i, uint8_t *canvas, int x, int y)
{
    if (!m || i < 0 || i >= m->n || !m->font)
        return;
    const row *r = &m->rows[i];
    int v = r->value ? *r->value : 0;
    if (r->kind == R_TOGGLE)
        dk_fw_text_draw_box(canvas, m->font, x + 0x4e, y + 2, 2, 0, 0, "%s", v ? "ON" : "OFF");
    else if (r->kind == R_VALUE)
        dk_fw_text_draw_box(canvas, m->font, x + 0x4e, y + 2, 2, 0, 0, "%03d", v);
}

static void ui_action(dk_ui_menu *m, const char *text, dk_ui_action_fn *fn_, void *ctx, int arg)
{
    add(m, text, R_ACTION, fn_, ctx, arg, 0);
}

static void ui_submenu(dk_ui_menu *m, const char *text, dk_ui_menu *sub)
{
    if (!sub)
        return;
    if (m) {
        row *r = add_row(m);
        if (!r)
            return;
        r->kind = R_SUBMENU;
        r->sub = sub;
        m->n++;
        add_item(m, text, m->n - 1, 0);
        invalidate(m);
    }
}

static void ui_text(dk_ui_menu *m, const char *text)
{
    add(m, text, R_TEXT, 0, 0, 0, 0);
}

static void ui_toggle(dk_ui_menu *m, const char *text, int *value, dk_ui_action_fn *changed,
                      void *ctx)
{
    if (!m)
        return;
    row *r = add_row(m);
    if (!r)
        return;
    r->kind = R_TOGGLE;
    r->value = value;
    r->fn = changed;
    r->ctx = ctx;
    m->n++;
    add_item(m, text, m->n - 1, 1);
    invalidate(m);
}

static void ui_value(dk_ui_menu *m, const char *text, int *value, int min, int max,
                     dk_ui_action_fn *changed, void *ctx)
{
    if (!m)
        return;
    row *r = add_row(m);
    if (!r)
        return;
    r->kind = R_VALUE;
    r->value = value;
    r->min = min;
    r->max = max;
    r->fn = changed;
    r->ctx = ctx;
    m->n++;
    add_item(m, text, m->n - 1, 1);
    invalidate(m);
}

/* ---- own View vtables ------------------------------------------------------------------------------------------------- */

static uint32_t g_text_vt[VT_WORDS];
static uint32_t g_list_vt[VT_WORDS];
static uint32_t g_text_stock_paint;
static uint32_t g_list_stock_paint;
static uint32_t g_list_stock_key;

/* The stock View vtable slots 2 and 4 of these views are thunks that start with `moveq #-0x38,d0` (0x70c8). */
static int is_thunk(uint32_t addr)
{
    return *(const uint16_t *)addr == 0x70c8;
}

static int rows_of(const uint8_t *obj)
{
    return (int)((read_u32(obj + offsetof(CompactVerticalMenuView, items_end)) -
                  read_u32(obj + offsetof(CompactVerticalMenuView, items_begin))) >>
                 3);
}

/* A text page paints without the highlighted row: the cursor is set out of range around the stock paint. */
static void text_paint(uint8_t *view, uint8_t *canvas)
{
    uint32_t *cursor = (uint32_t *)(view - VIEW_AT + offsetof(VerticalMenuView, cursor));
    uint32_t saved = *cursor;
    *cursor = 0x7fffffff;
    ((void (*)(uint8_t *, uint8_t *))g_text_stock_paint)(view, canvas);
    *cursor = saved;
}

/* A list with no rows. The firmware's list code is not made for an empty Menu (its paint, LED and key code read the item at the
   selection), so an empty list keeps one hidden item (see refill) and its paint is ours: the title bar and the descriptor's empty
   text, centred. The bar is drawn as the stock paint draws it: clear the bar, the title (flags 2 at x 0x40, y 0x41 minus the field at
   +0x84, which is 7), the two side lines, then an invert fill of the inside (y counts from the bottom). */
static void list_paint(uint8_t *view, uint8_t *canvas)
{
    uint8_t *obj = view - VIEW_AT;
    dk_ui_menu *m = *(dk_ui_menu **)(obj + POP_EXT_AT);
    if (!m || !m->d || m->n != 0) {
        ((void (*)(uint8_t *, uint8_t *))g_list_stock_paint)(view, canvas);
        return;
    }
    const CompactVerticalMenuView *c = (const CompactVerticalMenuView *)obj;
    int d = (int)read_u32(obj + 0x84);
    int x0 = c->x, x1 = c->x + (int)c->width - 1;
    const char *title = (const char *)c->title;
    dk_fw_draw_fill_rect(canvas, 0, 0x3f, 0x7f, 0x40 - d, 0);
    if (title)
        dk_fw_text_draw_fmt(canvas, obj + CMP_FONT_AT, 0x40, 0x41 - d, 2, "%s", title);
    dk_fw_draw_fill_rect(canvas, x0, 0x3e, x0, 0x41 - d, -1);
    dk_fw_draw_fill_rect(canvas, x1, 0x3e, x1, 0x41 - d, -1);
    dk_fw_draw_fill_rect(canvas, x0 + 1, 0x3f, x1 - 1, 0x40 - d, -1);
    if (m->d->empty)
        dk_fw_text_draw_fmt(canvas, obj + CMP_FONT_AT, 0x40, 0x1c, 2, "%s", m->d->empty);
}

static void toggle_check(dk_ui_menu *m, int row_);

static void open_panel(dk_ui_menu *m)
{
    dk_ui_menu *pm = m->panel;
    uint8_t *obj = m->obj;
    int row_ = (int)((MenuWithPopoutSideBarsView *)obj)->main.selected;
    if (!pm || !m->d->panel || row_ < 0 || row_ >= m->n)
        return;
    dk_fw_menu_clear_items(pm->obj);
    pm->n = 0;
    pm->filling = 1;
    m->d->panel(pm, row_, m->d->arg);
    pm->filling = 0;
    if (pm->n > 0)
        dk_fw_menu_popout_set_active(obj, pm->obj);
}

/* Keys of a list. Everything not handled here goes to the stock handler (focus moves, cursor, NO closes). */
static uint32_t list_key(uint8_t *view, uint8_t *ev)
{
    uint8_t *obj = view - VIEW_AT;
    dk_ui_menu *m = *(dk_ui_menu **)(obj + POP_EXT_AT);
    MenuWithPopoutSideBarsView *p = (MenuWithPopoutSideBarsView *)obj;
    uint32_t id = read_u32(ev + 0xc), fl = read_u32(ev + 0x10);
    if (m && m->d && p->active == (uint32_t)obj) {
        if (id == KEY_NO && (fl & KEYFL_RELEASE) && m->d->back && m->d->back(m->d->arg) == 1)
            return 1;
        /* only the hidden item: YES and RIGHT have nothing to act on or open; every other key (NO, the cursor keys, an unknown one) is the stock handler's */
        if (m->n == 0 && (id == KEY_YES || id == KEY_YES2 || id == KEY_RIGHT))
            return 1;
        if (m->n == 0)
            return ((uint32_t (*)(uint8_t *, uint8_t *))g_list_stock_key)(view, ev);
        if (id == KEY_RIGHT && (fl & KEYFL_PRESS) && m->d->panel && p->has_right) {
            open_panel(m);
            return 1;
        }
        if ((id == KEY_YES || id == KEY_YES2) && (fl & KEYFL_RELEASE) && m->d->check) {
            int row_ = (int)p->main.selected;
            if (row_ >= 0 && row_ < m->n)
                toggle_check(m, row_);
            return 1;
        }
    }
    return ((uint32_t (*)(uint8_t *, uint8_t *))g_list_stock_key)(view, ev);
}

/* Copies the View vtable of `obj` into `copy` and replaces slot 4 (and slot 2 when key is not 0). 0 when a slot is not the thunk. */
static int own_vtable(uint8_t *obj, uint32_t *copy, uint32_t *stock_paint, uint32_t *stock_key,
                      void *paint, void *key)
{
    uint32_t *old = *(uint32_t **)(obj + VIEW_AT);
    if (old == &copy[2])
        return 1;
    if (!*stock_paint) {
        if (!is_thunk(old[VT_PAINT]) || (key && !is_thunk(old[VT_KEY])))
            return 0;
        for (int i = 0; i < VT_WORDS; i++)
            copy[i] = old[i - 2];
        *stock_paint = copy[2 + VT_PAINT];
        copy[2 + VT_PAINT] = (uint32_t)paint;
        if (key) {
            *stock_key = copy[2 + VT_KEY];
            copy[2 + VT_KEY] = (uint32_t)key;
        }
    }
    *(uint32_t **)(obj + VIEW_AT) = &copy[2];
    return 1;
}

/* ---- menus ------------------------------------------------------------------------------------------------------------ */

static int build_menu(dk_ui_menu *m, const dk_ui_menu_desc *d)
{
    uint32_t title;
    uint8_t alloc = 0;
    uint8_t *obj;
    dk_fw_string_std_string_ctor(&title, d->title ? d->title : "", &alloc);
    if (m->kind == K_LIST) {
        obj = fw_new(POP_EXT_AT + 8);
        if (!obj)
            goto fail;
        dk_fw_menu_popout_ctor(obj, d->name, 0x40, 0, 1, 1);
        dk_fw_menu_set_title(obj, &title);
        m->font = obj + CMP_FONT_AT;
        *(dk_ui_menu **)(obj + POP_EXT_AT) = m;
        m->obj = obj;
        if (!own_vtable(obj, g_list_vt, &g_list_stock_paint, &g_list_stock_key, (void *)list_paint,
                        (void *)list_key))
            goto fail;
        dk_ui_menu *pm = (dk_ui_menu *)fw_new(sizeof(dk_ui_menu));
        if (!pm)
            goto fail;
        pm->kind = K_PANEL;
        pm->obj = obj + POP_PANEL_AT;
        pm->font = obj + CMP_FONT_AT;
        pm->owner = m;
        m->panel = pm;
    } else {
        obj = fw_new(sizeof(VerticalMenuView));
        if (!obj)
            goto fail;
        dk_fw_menu_vertical_menu_view_ctor(obj, d->name, 8, &title, 0);
        m->obj = obj;
        m->font = vmv_font();
        if (m->kind == K_TEXT &&
            !own_vtable(obj, g_text_vt, &g_text_stock_paint, 0, (void *)text_paint, 0))
            goto fail;
    }
    dk_fw_string_std_string_dtor(&title);
    m->sp = (shared_ptr *)fw_new(sizeof(shared_ptr));
    if (!m->sp)
        return 0;
    dk_make_sp(m->sp, g_parent, obj + VIEW_AT);
    return 1;
fail:
    dk_fw_string_std_string_dtor(&title);
    return 0;
}

dk_ui_menu *dk_ui_menu_new(dk_ui *ui, const dk_ui_menu_desc *d)
{
    (void)ui;
    if (!d)
        return 0;
    if (!g_parent) {
        g_parent = fw_new(0xa0);
        if (!g_parent)
            return 0;
    }
    dk_ui_menu *m = (dk_ui_menu *)fw_new(sizeof(dk_ui_menu));
    if (!m)
        return 0;
    m->d = d;
    m->kind = d->flags & (DK_UI_LIST | DK_UI_WIDE) ? K_LIST
              : d->flags & DK_UI_TEXT_PAGE         ? K_TEXT
                                                   : K_MENU;
    if (!build_menu(m, d))
        return 0; /* the partly built objects stay: the firmware heap is not given back */
    /* the menu keeps d, its fill and its arg: the runtime does not unload or reload the module that owns them */
    dkrt_menu_built((uintptr_t)d->fill, (uintptr_t)d);
    return m;
}

/* Fill again: the selection stays on the same row number when it can, or goes to the top (`top`). */
static void refill(dk_ui_menu *m, int top)
{
    if (m->filling)
        return;
    int sel = top ? 0 : (int)((CompactVerticalMenuView *)m->obj)->selected;
    dk_fw_menu_clear_items(m->obj);
    m->n = 0;
    m->filling = 1;
    if (m->d->fill)
        m->d->fill(m, m->d->arg);
    m->filling = 0;
    if (m->n == 0 && m->kind == K_LIST)
        add_item(m, "", 0,
                 0); /* the hidden item of an empty list: m->n stays 0, so run_row ignores it */
    int count = rows_of(m->obj);
    if (count > 0) {
        if (sel >= count)
            sel = count - 1;
        if (sel < 0)
            sel = 0;
        ((void (*)(uint8_t *, uint32_t, uint32_t))vtable(m->obj)[7])(m->obj, (uint32_t)sel, 2);
    }
    invalidate(m);
}

static int ui_is_open(const dk_ui_menu *m)
{
    if (!m || !m->obj || m->kind == K_PANEL)
        return 0;
    const uint8_t *v = m->obj + VIEW_AT;
    return read_u32(v + offsetof(View, mgr)) != 0 && v[0x30] == 0;
}

static int ui_open(dk_ui_menu *m)
{
    if (!m || !m->obj || !m->d || m->kind == K_PANEL)
        return -1;
    refill(m, m->kind == K_TEXT && !ui_is_open(m)); /* a text page opens at its first row */
    if (ui_is_open(m))
        return 0;
    return dk_fw_view_view_controller_push(view_manager(), m->sp, 0) ? 0 : -1;
}

static void ui_refresh(dk_ui_menu *m)
{
    if (m && m->obj && m->d && m->kind != K_PANEL)
        refill(m, 0);
}

static void ui_select(dk_ui_menu *m, int row_)
{
    if (!m || !m->obj || !m->d || m->kind == K_PANEL || row_ < 0)
        return;
    ((CompactVerticalMenuView *)m->obj)->selected =
        (uint32_t)row_; /* the next fill puts the cursor there (clamped to the rows) */
    if (ui_is_open(m))
        refill(m, 0);
}

static void ui_close(dk_ui_menu *m)
{
    if (!ui_is_open(m))
        return;
    uint8_t *v = m->obj + VIEW_AT;
    ((void (*)(uint8_t *))vtable(v)[VT_CLOSE])(v);
}

/* ---- check marks ------------------------------------------------------------------------------------------------------ */

static int ui_checked(const dk_ui_menu *list, int row_)
{
    if (!list || list->kind != K_LIST || row_ < 0)
        return 0;
    return dk_fw_menu_is_checked(list->obj, row_) & 1;
}

static void ui_check(dk_ui_menu *list, int row_, int on)
{
    if (!list || list->kind != K_LIST || row_ < 0)
        return;
    int cur = ui_checked(list, row_);
    if (on && !cur)
        dk_fw_menu_check(list->obj, row_);
    else if (!on && cur)
        dk_fw_menu_uncheck(list->obj, row_);
    invalidate(list);
}

static void toggle_check(dk_ui_menu *m, int row_)
{
    int on = !ui_checked(m, row_);
    ui_check(m, row_, on);
    if (m->d->check)
        m->d->check(m->d->arg, row_, on);
}

/* The owner of what a module or a service file hands in: its handle, which dk_ui_open made from the owner id of its record (and dk_ui_core_handle for core modules). */
static uintptr_t owner_of(dk_ui *ui)
{
    return (uintptr_t)ui;
}

/* ---- dialogs ---------------------------------------------------------------------------------------------------------- */

static ui_dialog_slot g_dialog; /* the one open dialog */
static uint32_t g_serial;

/* the answer: the window closed itself first, then the slot frees itself and runs the asker's function if the asker is still loaded */
static void inv_answer(const uint32_t *any, int arg)
{
    (void)any;
    ui_dialog_answer(&g_dialog, arg);
}

static void inv_answer_bool(const uint32_t *any, int arg)
{
    inv_answer(any, (arg & 0xff) != 0);
}

/* the window left the view stack (detach, with the manager and the view as arguments): any way of closing ends the dialog */
static void inv_detach(const uint32_t *any)
{
    ui_dialog_detach(&g_dialog, any[0]);
}

static void on_detach(uint8_t *view, uint32_t serial)
{
    fn f = make_fn(serial, 0, inv_detach);
    ((void (*)(uint8_t *, fn *))vtable(view)[VT_DETACH_ADD])(view, &f);
}

/* The make_shared block header: the control-block vtable, use and weak count 1, the object pointer (the object is at +0x10). */
static void init_block(uint8_t *blk, const uint8_t *cb_vt)
{
    uint32_t *b = (uint32_t *)blk;
    b[0] = (uint32_t)cb_vt;
    b[1] = 1;
    b[2] = 1;
    b[3] = (uint32_t)blk + 0x10;
}

/* A make_shared block for an object of `obj_bytes` bytes: the 0x10 control block with `cb_vt`, the object at +0x10; zeroed, from the firmware heap. 0 when out of memory. */
static uint8_t *new_block(uint32_t obj_bytes, const uint8_t *cb_vt)
{
    uint8_t *blk = fw_new(0x10 + obj_bytes);
    if (blk)
        init_block(blk, cb_vt);
    return blk;
}

/* Pushes the object of a block on the view manager (the shared_ptr is the block's object and the block); our count of 1 is released, the stack's copy keeps
   the view and the firmware frees it after its sweep. 1 when pushed. */
static int push_shared(uint8_t *blk)
{
    shared_ptr sp = {(uint32_t)blk + 0x10, (uint32_t)blk};
    uint32_t ok = dk_fw_view_view_controller_push(view_manager(), &sp, 0);
    dk_fw_string_shared_ptr_release(&sp.ctl);
    return ok != 0;
}

/* A dialog: its detach callback (any way of closing ends the dialog), then the push; 0 when shown. */
static int show(uint8_t *blk, uint32_t serial)
{
    on_detach(blk + 0x10, serial);
    return push_shared(blk) ? 0 : -1;
}

int32_t dk_ui_confirm(dk_ui *ui, const char *title, const char *text, dk_ui_confirm_fn *done,
                      void *arg)
{
    if (ui_dialog_is_open(&g_dialog))
        return -1;
    uint8_t *blk = new_block(0x7c, &fw_confirm_cb_vt);
    if (!blk)
        return -1;
    char t[81], x[81];
    cut(t, sizeof t, title);
    cut(x, sizeof x, text);
    uint8_t alloc = 0;
    uint32_t st, sx;
    dk_fw_string_std_string_ctor(&st, t, &alloc);
    dk_fw_string_std_string_ctor(&sx, x, &alloc);
    fn cb = make_fn(0, 0, inv_answer_bool);
    uint32_t serial = ++g_serial;
    dk_fw_window_confirm_ctor(blk + 0x10, &st, &sx, (const std_function *)&cb, 0);
    dk_fw_string_std_string_dtor(&sx);
    dk_fw_string_std_string_dtor(&st);
    ui_dialog_begin(&g_dialog, serial, owner_of(ui), done, arg);
    if (show(blk, serial) != 0) {
        ui_dialog_detach(&g_dialog, serial);
        return -1;
    }
    return 0;
}

/* The text of a choice item, up to DK_UI_LIST_TEXT - 1 characters. An item "NAME\tREST" has its REST start at pixel `col` (the width of the longest NAME of the
   choice; with col -1 only the NAME is written, to measure it), so the rests of the items line up in the proportional list font (to the nearest 3 pixels: the popup draws a box for the PAD character). */
static void choice_item(char *buf, uint32_t size, const char *item, int col)
{
    uint32_t n = 0;
    while (item && item[n] && item[n] != '\t' && n + 1 < size) {
        buf[n] = item[n];
        n++;
    }
    buf[n] = 0;
    if (!item || item[n] != '\t' || col < 0)
        return;
    const char *r = item + n + 1;
    n = (uint32_t)dk_ui_pad_spaces(buf, (int)n, (int)size, col);
    for (; *r && n + 1 < size; r++)
        buf[n++] = *r;
    buf[n] = 0;
}

int32_t dk_ui_choice(dk_ui *ui, const char *title, const char *const *items, int32_t n,
                     dk_ui_choice_fn *done, void *arg)
{
    if (ui_dialog_is_open(&g_dialog) || !items || n <= 0 || n > MAX_CHOICE)
        return -1;
    uint8_t *blk = new_block(0xd8, &fw_choice_cb_vt);
    uint32_t *strs = (uint32_t *)fw_new(4 * (uint32_t)n);
    if (!blk || !strs)
        return -1;
    uint8_t alloc = 0;
    char buf[DK_UI_LIST_TEXT];
    int col = 0;
    for (int i = 0; i < n; i++) {
        choice_item(buf, sizeof buf, items[i], -1);
        if (dk_ui_px_of(buf) > col)
            col = dk_ui_px_of(buf);
    }
    for (int i = 0; i < n; i++) {
        choice_item(buf, sizeof buf, items[i], col);
        dk_fw_string_std_string_ctor(&strs[i], buf, &alloc);
    }
    uint32_t vec[3] = {(uint32_t)strs, (uint32_t)(strs + n), (uint32_t)(strs + n)};
    uint32_t st;
    cut(buf, DK_UI_ROW, title);
    dk_fw_string_std_string_ctor(&st, buf, &alloc);
    fn cb = make_fn(0, 0, inv_answer);
    uint32_t serial = ++g_serial;
    dk_fw_window_multichoice_ctor(blk + 0x10, vec, &st, (const std_function *)&cb, 1);
    dk_fw_string_std_string_dtor(&st);
    for (int i = 0; i < n; i++)
        dk_fw_string_std_string_dtor(&strs[i]);
    dk_fw_mem_free((uint8_t *)strs);
    ui_dialog_begin(&g_dialog, serial, owner_of(ui), done, arg);
    if (show(blk, serial) != 0) {
        ui_dialog_detach(&g_dialog, serial);
        return -1;
    }
    return 0;
}

int32_t dk_ui_toast(dk_ui *ui, const char *text)
{
    (void)ui;
    char buf[DK_UI_TOAST + 1];
    cut(buf, sizeof buf, text);
    dk_fw_window_toast(dk_fw_window_ui_states(), "%s", buf);
    return 0;
}

/* ---- dk_ui_tick ------------------------------------------------------------------------------------------------------- */

/* The timers (tick.h). One function, ui_poll, is on the runtime's per-pass list (dkrt_poll_add) while any timer runs: the UI loop calls it
   in the UI task, the task of every UI callback. The runtime's block clock (1500 per second) times the periods; without one the passes
   of the UI loop (about 120 per second) do. */
static ui_ticks g_ticks;
static uint32_t g_passes;
static int g_polling;

static uint32_t tick_clock(uint32_t *per_s)
{
    uint32_t now = dkrt_blocks();
    if (now) {
        *per_s = 1500;
        return now;
    }
    *per_s = 120;
    return ++g_passes;
}

static void ui_poll(void);
static ui_views g_views;
static ui_defer g_defer;
static ui_settings_rows g_rows;
static ui_text_input g_text;

static uint32_t views_used(void)
{
    return ui_views_used(&g_views);
}

/* On the runtime's per-pass list while a timer runs, a module view is open or the stock SETTINGS list waits for a rebuild. */
static int poll_start(void)
{
    if (g_polling)
        return 0;
    if (dkrt_poll_add((uintptr_t)ui_poll) != 0)
        return -1;
    g_polling = 1;
    return 0;
}

/* The key state for the actions that wait for a release: the firmware's key_down takes the code of the key event. */
static int key_is_down(void *unused, uint32_t code)
{
    (void)unused;
    return dk_fw_input_key_down(code) != 0;
}

static void ui_poll(void)
{
    uint32_t per_s, now = tick_clock(&per_s);
    uint32_t timers = ui_ticks_run(&g_ticks, now, per_s);
    ui_defer_poll(&g_defer, key_is_down, 0);
    if (g_views.fw)
        ui_views_poll(&g_views);
    ui_rows_pass(&g_rows, dk_fw_menu_settings_list_rebuild);
    if (timers == 0 && views_used() == 0 && ui_defer_pending(&g_defer) == 0 &&
        !ui_rows_dirty(&g_rows) && g_polling) {
        dkrt_poll_remove((uintptr_t)ui_poll);
        g_polling = 0;
    }
}

int32_t dk_ui_tick(dk_ui *ui, dk_ui_tick_fn *fn, void *arg, uint32_t period_ms)
{
    uint32_t per_s, now = tick_clock(&per_s);
    int r = ui_ticks_set(&g_ticks, owner_of(ui), fn, arg, period_ms, now);
    if (r == 0 && ui_ticks_active(&g_ticks) && poll_start() != 0) {
        ui_ticks_set(&g_ticks, 0, fn, arg, 0, now);
        return -1;
    }
    return r;
}

/* ---- module views, SETTINGS rows, text input ---------------------------------------------------------------------------
   The logic is in settings_rows.c, view_pool.c, canvas_map.c and text_input.c (host-tested). This section is the glue to the firmware. */

/* Called by the runtime (dkrt_unload_observer_add) with the owner id of a module or service file that is unloaded, after its fini and before its memory is freed: the
   record's views, SETTINGS rows and timers go. The owner key is the handle value (owner id + 1). */
static void ui_owner_gone(uint32_t owner)
{
    uintptr_t key = (uintptr_t)owner + 1;
    ui_rows_remove_owner(&g_rows, key);
    if (ui_rows_dirty(&g_rows))
        poll_start(); /* the stock list still shows the owner's row: the next pass builds it again */
    ui_dialog_drop_owner(&g_dialog, key);
    ui_views_close_owner(&g_views, key);
    ui_ticks_remove_owner(&g_ticks, key);
    ui_defer_drop_owner(&g_defer, key);
    ui_text_drop_owner(&g_text, key);
}

/* Watch for unloads, once. */
static int observe_unloads(void)
{
    static int done;
    if (done)
        return 0;
    if (dkrt_unload_observer_add((uintptr_t)ui_owner_gone) != 0)
        return -1;
    done = 1;
    return 0;
}

/* -- SETTINGS rows -- */

static void run_settings_row(void *key, int32_t unused)
{
    (void)unused;
    ui_rows_run(&g_rows, (const ui_row_key *)key);
}

/* Called from dk_hook (shim.s) after the app's rows. */
void ui_settings_rows_tail(uint8_t *settings_menu)
{
    for (uint32_t i = 0; i < g_rows.n; i++)
        dk_ui_stock_row(settings_menu, g_rows.row[i].text, run_settings_row,
                        (void *)g_rows.row[i].key);
    ui_rows_built(&g_rows, settings_menu); /* the list is the one the firmware builds for the session: ui_poll builds it again when the rows change */
}

int32_t dk_ui_settings_row(dk_ui *ui, const char *text, dk_ui_settings_open_fn *open, void *arg)
{
    char name[DK_UI_MODULE_NAME];
    ui_owner_name(owner_of(ui), dkrt_record_name, name);
    int32_t r = ui_rows_add(&g_rows, owner_of(ui), name, dkrt_phase() == DKRT_PHASE_INIT, text,
                            open, arg);
    if (r == 0 && ui_rows_dirty(&g_rows))
        poll_start(); /* a row added after the stock list was built: the next pass builds it again */
    return r;
}

/* -- canvas: the firmware side of canvas_map.h -- */

/* The fonts of the canvas by dk_font: the stock UI font and the large ROM font (anchor `cf.text.data.font_large`; a firmware without it draws the small one). */
static uint8_t *g_canvas_font[2];

/* The screen's own calls. Text y and rectangle rows both count from the screen bottom (the title bar of every stock list is drawn at rows 0x39 to 0x3f
   with the text at y 0x3a: checked in the stock paint code). `maxw` -1: the whole text (the main screen passes -1). A set pixel is colour -1, a clear
   one 0 (the stock paint calls), and the selection bar of the stock lists is inverted with colour -1. */
static void fwc_text(void *bitmap, int32_t font, int32_t x, int32_t y_up, const char *s)
{
    dk_fw_text_draw((uint8_t *)bitmap, g_canvas_font[font], x, y_up, -1, s);
}

static void fwc_fill(void *bitmap, int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color)
{
    dk_fw_draw_fill_rect((uint8_t *)bitmap, x0, y0, x1, y1, color ? -1 : 0);
}

static void fwc_invert(void *bitmap, int32_t x0, int32_t y0, int32_t x1, int32_t y1)
{
    dk_fw_draw_invert_rect((uint8_t *)bitmap, x0, y0, x1, y1, -1);
}

/* The large ROM font, or `small` on a firmware whose port has no `cf.text.data.font_large`: the anchor is optional and links 0 there. */
static uint8_t *large_font(uint8_t *small)
{
    uint8_t *volatile p = (uint8_t *)&dk_fw_text_font_large;
    return p ? p : small;
}

/* The anchors behind module screens (the text width, the View constructor, the naming screen and its callback vtable) are optional in module.toml: a firmware that
   lacks them links 0. A module's view or text input is then refused (`dk_ui_view_open` returns 0, `dk_ui_text_input` DK_E_CONTRACT) instead of calling address 0. The
   pointers go through volatile objects so that the compiler does not fold the test (a symbol's address is never 0 to it). */
static int fw_screens(void)
{
    void *volatile a = (void *)dk_fw_text_width;
    void *volatile b = (void *)dk_fw_view_view_ctor;
    void *volatile c = (void *)dk_fw_window_name_ctor;
    void *volatile d = (void *)&fw_name_cb_vt;
    return a && b && c && d;
}

/* The width routine takes the font data and a std::string (a pointer to the one-word COW object). */
static int32_t fwc_text_width(int32_t font, const char *s)
{
    uint32_t str;
    uint8_t alloc = 0;
    int32_t w;
    dk_fw_string_std_string_ctor(&str, s, &alloc);
    w = dk_fw_text_width(g_canvas_font[font], -1, &str);
    dk_fw_string_std_string_dtor(&str);
    return w;
}

/* Fills `fw` for the screen Bitmap of this frame: +4 width, +8 height (dk_fw_draw.h); the glyph box height of the font is FontData.height. */
static void canvas_fw_for(ui_canvas_fw *fw, uint8_t *bitmap)
{
    g_canvas_font[0] = vmv_font();
    g_canvas_font[1] = large_font(g_canvas_font[0]);
    fw->bitmap = bitmap;
    fw->width = read_u32(bitmap + 4);
    fw->height = read_u32(bitmap + 8);
    for (int i = 0; i < 2; i++)
        fw->font_h[i] = *(const uint16_t *)(g_canvas_font[i] + offsetof(FontData, height));
    fw->rect_from_bottom = 1;
    fw->text = fwc_text;
    fw->fill = fwc_fill;
    fw->invert = fwc_invert;
    fw->text_width = fwc_text_width;
}

/* -- views -- */

/* A module view is the stock View (the generated struct, size 0x4c: the rule cf.view.struct.View) and one word of ours behind it: its pool slot. */
#define VIEW_SLOT_AT sizeof(View)
#define VIEW_OBJ_SIZE (sizeof(View) + sizeof(uint32_t))

/* Slot 4 of the view vtable: draw(this, Bitmap *). */
static void mv_draw(uint8_t *view, uint8_t *bitmap)
{
    ui_canvas_fw fw;
    dk_canvas c;
    canvas_fw_for(&fw, bitmap);
    ui_canvas_bind(&c, &fw);
    ui_views_draw(&g_views, read_u32(view + VIEW_SLOT_AT), &c);
    ui_canvas_unbind();
}

/* Slot 2 of the view vtable: key(this, KeyEvent *): code at +0xc, flags at +0x10. Returns nonzero when the key is used. */
static uint32_t mv_key(uint8_t *view, uint8_t *ev)
{
    return (uint32_t)ui_views_key(&g_views, read_u32(view + VIEW_SLOT_AT), read_u32(ev + 0xc),
                                  read_u32(ev + 0x10));
}

/* The detach callback of the firmware view (registered through slot 16): the view left the stack. */
static void mv_detached(uint32_t slot)
{
    ui_views_gone(&g_views, slot);
}

/* The view left the stack: the sweep ran the detach callback. */
static void inv_view_detached(const uint32_t *any)
{
    mv_detached(any[0]);
}

/* Our own vtable: the View vtable (the vptr the constructor stored) copied, slot 2 (key) and 4 (draw) replaced. Slot 0 stays View's complete
   destructor and slot 1 its deleting one. One copy serves every module view. */
static uint32_t g_view_vt[VT_WORDS];

static void own_view_vtable(uint8_t *obj)
{
    uint32_t *old = (uint32_t *)vtable(obj);
    if (!g_view_vt[2]) {
        for (int i = 0; i < VT_WORDS; i++)
            g_view_vt[i] = old[i - 2];
        g_view_vt[2 + VT_PAINT] = (uint32_t)mv_draw;
        g_view_vt[2 + VT_KEY] = (uint32_t)mv_key;
    }
    *(uint32_t **)obj = &g_view_vt[2];
}

/* Make the firmware view for pool slot `slot` and push it, as the dialogs are: a make_shared block (0x10 control block, the object at +0x10, the
   control-block vtable of the stock ConfirmWindow, whose dispose calls slot 0 of the object and whose destroy frees the block), pushed on the view
   manager; our own count is released afterwards, so the firmware frees the view after its sweep. Opaque: a full-screen view hides the views below, an
   overlay does not. Returns the object as the opaque word, 0 when it could not be made. */
static uint32_t fwv_push(void *fw, uint32_t slot, int overlay)
{
    (void)fw;
    if (!fw_screens())
        return 0;
    uint8_t *blk = new_block(VIEW_OBJ_SIZE, &fw_confirm_cb_vt);
    if (!blk)
        return 0;
    uint8_t *obj = blk + 0x10;
    dk_fw_view_view_ctor(obj, "dkview"); /* the name is kept by pointer: static */
    own_view_vtable(obj);
    *(uint32_t *)(obj + VIEW_SLOT_AT) = slot;
    ((View *)obj)->opaque = overlay ? 0 : 1; /* before the push: the push runs the relayout */
    fn f = make_fn(slot, 0, inv_view_detached);
    ((void (*)(uint8_t *, fn *))vtable(obj)[VT_DETACH_ADD])(obj, &f);
    return push_shared(blk) ? (uint32_t)obj : 0;
}

/* Flag the view for removal: slot 10 (close). [D] only flags; the sweep removes it. */
static void fwv_close(void *fw, uint32_t view)
{
    (void)fw;
    uint8_t *v = (uint8_t *)view;
    ((void (*)(uint8_t *))vtable(v)[VT_CLOSE])(v);
}

static void fwv_invalidate(void *fw, uint32_t view)
{
    (void)fw;
    dk_fw_view_invalidate((uint8_t *)view);
}

static int32_t fwv_poll_start(void *fw)
{
    (void)fw;
    return poll_start();
}

/* A runtime log event of a module screen (`dk_log_event`); the owner handle is the first word. */
static void emit_event(uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a, uint32_t b)
{
    uint32_t args[5] = {(uint32_t)owner, a, b, 0, 0};
    dk_log_event(kind, flags, args);
}

static void fwv_emit(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a,
                     uint32_t b)
{
    (void)fw;
    emit_event(kind, flags, owner, a, b);
}

static void fwt_emit(void *fw, uint32_t kind, uint32_t flags, uintptr_t owner, uint32_t a)
{
    (void)fw;
    emit_event(kind, flags, owner, a, 0);
}

static const ui_view_fw g_view_fw = {0,         fwv_poll_start, fwv_push,
                                     fwv_close, fwv_invalidate, fwv_emit};

/* The queue of actions that wait for a key release starts the per-pass poll when it gets one. */
static int32_t defer_start(void *unused)
{
    (void)unused;
    return poll_start();
}

static void defer_init(void)
{
    g_defer.start = defer_start;
}

dk_ui_view *dk_ui_view_open(dk_ui *ui, const dk_ui_view_desc *d)
{
    defer_init();
    if (!g_views.fw)
        ui_views_init(&g_views, &g_view_fw, &g_defer);
    return (dk_ui_view *)ui_views_open(&g_views, owner_of(ui), d);
}

void dk_ui_view_close(dk_ui_view *v)
{
    if (g_views.fw)
        ui_views_close(&g_views, (uintptr_t)v);
}

/* -- text input -- */

/* How many characters the NameView edit box takes without a number prefix: the largest maxlen that a stock call site passes (16, in 21 call sites of the
   NameView wrapper); no fw rule holds it, it is evidence from those sites. The API cap (15) is below it. */
#define NAME_SCREEN_MAX 16u

/* The firmware's `done`, a std::function<void(std::string, int)>: invoker(storage, std::string *, number) on YES, before the screen closes. The text is
   the COW string's characters, trailing blanks already removed. */
static void inv_name_done(const uint32_t *any, const uint32_t *str, int number)
{
    (void)any;
    (void)number;
    ui_text_done(&g_text, (const char *)*str);
}

/* Any way of closing ends the input (NO has no done). */
static void inv_name_detached(const uint32_t *any)
{
    ui_text_gone(&g_text, any[0]);
}

/* NameView(this, prompt, initial, number 0, maxlen, show_number 0, number_fmt "", done) in a make_shared block, pushed as the dialogs are. 0 when shown. */
static int32_t fwt_push(void *fw, const dk_ui_text_input_desc *d, const char *initial,
                        uint32_t serial)
{
    (void)fw;
    if (!fw_screens())
        return DK_E_CONTRACT;
    uint8_t *blk = new_block(sizeof(NameView), &fw_name_cb_vt);
    if (!blk)
        return DK_E_NOMEM;
    uint8_t *obj = blk + 0x10;
    uint8_t alloc = 0;
    uint32_t prompt, start, fmt;
    char title[DK_UI_ROW + 1];
    cut(title, sizeof title, d->title);
    dk_fw_string_std_string_ctor(&prompt, title, &alloc);
    dk_fw_string_std_string_ctor(&start, initial, &alloc);
    dk_fw_string_std_string_ctor(&fmt, "", &alloc);
    fn done = make_fn(0, 0, inv_name_done);
    dk_fw_window_name_ctor(obj, &prompt, &start, 0, (int32_t)d->max_len, 0, &fmt,
                           (const std_function *)&done);
    dk_fw_string_std_string_dtor(&fmt);
    dk_fw_string_std_string_dtor(&start);
    dk_fw_string_std_string_dtor(&prompt);
    fn gone = make_fn(serial, 0, inv_name_detached);
    ((void (*)(uint8_t *, fn *))vtable(obj)[VT_DETACH_ADD])(obj, &gone);
    return push_shared(blk) ? 0 : DK_E_NOMEM;
}

static const ui_text_fw g_text_fw = {0, fwt_push, fwt_emit};

int32_t dk_ui_text_input(dk_ui *ui, const dk_ui_text_input_desc *d)
{
    defer_init();
    if (!g_text.fw) {
        g_text.fw = &g_text_fw;
        g_text.defer = &g_defer;
    }
    return ui_text_open(&g_text, owner_of(ui), d, NAME_SCREEN_MAX);
}

/* ---- the service imports of <dk/gen/imports.h> (docs/plans/sdk.md section 5.1) ----
   The menu calls: thin functions over the ui_* functions above. The dialogs, dk_ui_menu_new and dk_ui_tick are defined where they are implemented. */

static int ui_token; /* the dk_ui handle: only a token, there is one UI service per image */

dk_ui *dk_ui_core_handle(void)
{
    return (dk_ui *)&ui_token;
}

int32_t dk_ui_open(dk_module_ctx *ctx, dk_ui **out)
{
    if (!ctx || !out)
        return DK_E_CONTRACT;
    int32_t owner = dkrt_ctx_owner((uintptr_t)ctx);
    if (owner < 0)
        return DK_E_CONTRACT;
    if (observe_unloads() != 0)
        return DK_E_NOMEM;
    *out =
        (dk_ui *)((uintptr_t)owner + 1); /* the handle names the owner; it is never dereferenced */
    return 0;
}

int32_t dk_ui_menu_open(dk_ui_menu *m)
{
    return ui_open(m);
}

void dk_ui_menu_close(dk_ui_menu *m)
{
    ui_close(m);
}

void dk_ui_menu_select(dk_ui_menu *m, int32_t row)
{
    ui_select(m, (int)row);
}

void dk_ui_menu_refresh(dk_ui_menu *m)
{
    ui_refresh(m);
}

int32_t dk_ui_menu_is_open(const dk_ui_menu *m)
{
    return ui_is_open(m);
}

void dk_ui_menu_add_text(dk_ui_menu *m, const char *text)
{
    ui_text(m, text);
}

void dk_ui_menu_add_action(dk_ui_menu *m, const char *text, dk_ui_action_fn *run, void *arg,
                           int32_t value)
{
    ui_action(m, text, (dk_ui_action_fn *)run, arg, (int)value);
}

void dk_ui_menu_add_submenu(dk_ui_menu *m, const char *text, dk_ui_menu *sub)
{
    ui_submenu(m, text, sub);
}

void dk_ui_menu_add_toggle(dk_ui_menu *m, const char *text, int32_t *value,
                           dk_ui_action_fn *changed, void *arg)
{
    ui_toggle(m, text, (int *)value, (dk_ui_action_fn *)changed, arg);
}

void dk_ui_menu_add_value(dk_ui_menu *m, const char *text, int32_t *value, int32_t min, int32_t max,
                          dk_ui_action_fn *changed, void *arg)
{
    ui_value(m, text, (int *)value, (int)min, (int)max, (dk_ui_action_fn *)changed, arg);
}

void dk_ui_menu_check(dk_ui_menu *list, int32_t row, int32_t on)
{
    ui_check(list, (int)row, (int)on);
}

int32_t dk_ui_menu_checked(const dk_ui_menu *list, int32_t row)
{
    return ui_checked(list, (int)row);
}
