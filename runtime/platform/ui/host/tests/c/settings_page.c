/* Host test of the settings page (sdk/src/settings/page.c): its source, included so that sp_reset can clear its statics, with the fake settings store
   (settings_store.c) and the fake <dk/platform/ui.h> (src/lib.rs). settings_page.rs presses the keys. field.c is linked. */
#include <stdint.h>
#include <string.h>
#include "../../../../../../sdk/src/settings/page.c"

void fs_reset(void);
void fs_put_result(int rc);
int fs_puts(void);

static int ui_token, settings_token;
static struct {
    int calls;
    uint32_t field;
    int32_t value;
    int puts_at_call, fills, fills_at_call;
} ch;

static const char *const MODES[] = {"OFF", "SLOW", "FAST"};
static const dk_settings_field GOOD[] = {
    {.key = "loud", .label = "LOUD", .kind = DK_SETTINGS_BOOL, .def = 1},
    {.key = "gain", .label = "GAIN", .kind = DK_SETTINGS_INT, .def = 50, .min = 0, .max = 100},
    {.key = "trim", .label = "TRIM", .kind = DK_SETTINGS_INT, .def = -5, .min = -10, .max = 10},
    {.key = "mode", .label = "MODE", .kind = DK_SETTINGS_ENUM, .def = 1, .max = 2, .labels = MODES},
    {.key = "boot", .label = "BOOT FAST", .kind = DK_SETTINGS_BOOL, .flags = DK_SETTINGS_RESTART},
};
static const dk_settings_field NO_KEY[] = {
    {.key = "", .label = "A", .kind = DK_SETTINGS_BOOL}};
static const dk_settings_field SLASH_KEY[] = {
    {.key = "a/b", .label = "A", .kind = DK_SETTINGS_BOOL}};
static const dk_settings_field DOTS_KEY[] = {
    {.key = "a..b", .label = "A", .kind = DK_SETTINGS_BOOL}};
static const dk_settings_field SAME_KEY[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_BOOL},
    {.key = "a", .label = "B", .kind = DK_SETTINGS_BOOL}};
static const dk_settings_field BAD_BOOL_DEF[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_BOOL, .def = 2}};
static const dk_settings_field BAD_INT_RANGE[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_INT, .min = 0, .max = 1000}};
static const dk_settings_field BAD_INT_DEF[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_INT, .def = 11, .min = 0, .max = 10}};
static const dk_settings_field ENUM_NO_LABELS[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_ENUM, .max = 1}};
static const dk_settings_field ENUM_ONE_OPTION[] = {
    {.key = "a", .label = "A", .kind = DK_SETTINGS_ENUM, .max = 0, .labels = MODES}};
static const char *const LONG_OPTION[] = {"ABCDEFGHIJKLMNOPQRS", "B"};
static const dk_settings_field ENUM_ROW_TOO_WIDE[] = {
    {.key = "a", .label = "MODE", .kind = DK_SETTINGS_ENUM, .max = 1, .labels = LONG_OPTION}};
static const dk_settings_field BAD_KIND[] = {{.key = "a", .label = "A", .kind = 9}};

static void on_changed(void *arg, uint32_t field, int32_t value)
{
    (void)arg;
    ch.calls++;
    ch.field = field;
    ch.value = value;
    ch.puts_at_call = fs_puts();
    ch.fills_at_call = ch.fills;
}

static dk_settings_page_desc desc = {.fields = GOOD, .count = 5, .changed = on_changed};

void sp_reset(void)
{
    fs_reset();
    memset(&ch, 0, sizeof ch);
    g_desc = 0;
    g_settings = 0;
    g_ui = 0;
    g_menu = 0;
    memset(g_value, 0, sizeof g_value);
    memset(g_saved, 0, sizeof g_saved);
    g_failed = g_restart = 0;
}

void sp_store(const char *key, const char *text)
{
    dk_settings_put((dk_settings *)&settings_token, key, text, (uint32_t)strlen(text));
}

const char *sp_stored(const char *key)
{
    static char out[16];
    int32_t n = dk_settings_get((dk_settings *)&settings_token, key, out, sizeof out - 1);
    if (n < 0)
        return 0;
    out[n] = 0;
    return out;
}

int sp_init(void)
{
    return (int)dk_settings_page_init(&desc, (dk_settings *)&settings_token, (dk_ui *)&ui_token);
}
int sp_init_without(int settings, int ui)
{
    return (int)dk_settings_page_init(&desc, settings ? (dk_settings *)&settings_token : 0,
                                      ui ? (dk_ui *)&ui_token : 0);
}
/* an init with only the first (BOOL) field and no ui handle */
int sp_init_bool_only(void)
{
    dk_settings_page_desc d = {.fields = GOOD, .count = 1};
    return (int)dk_settings_page_init(&d, (dk_settings *)&settings_token, 0);
}

static void fill_page(dk_ui_menu *m, void *arg)
{
    (void)arg;
    ch.fills++;
    dk_settings_page_fill(m);
}
static const dk_ui_menu_desc page = {
    .name = "SettingsPage", .title = "TEST", .flags = DK_UI_WIDE, .fill = fill_page};

dk_ui_menu *sp_page(void)
{
    return dk_ui_menu_new((dk_ui *)&ui_token, &page);
}

int sp_check_case(int n)
{
    switch (n) {
    case 0: return (int)dk_settings_fields_check(GOOD, 5);
    case 1: return (int)dk_settings_fields_check(GOOD, 0);
    case 2: return (int)dk_settings_fields_check(GOOD, DK_SETTINGS_FIELDS + 1);
    case 3: return (int)dk_settings_fields_check(0, 1);
    case 4: return (int)dk_settings_fields_check(NO_KEY, 1);
    case 5: return (int)dk_settings_fields_check(SLASH_KEY, 1);
    case 6: return (int)dk_settings_fields_check(DOTS_KEY, 1);
    case 7: return (int)dk_settings_fields_check(SAME_KEY, 2);
    case 8: return (int)dk_settings_fields_check(BAD_BOOL_DEF, 1);
    case 9: return (int)dk_settings_fields_check(BAD_INT_RANGE, 1);
    case 10: return (int)dk_settings_fields_check(BAD_INT_DEF, 1);
    case 11: return (int)dk_settings_fields_check(ENUM_NO_LABELS, 1);
    case 12: return (int)dk_settings_fields_check(ENUM_ONE_OPTION, 1);
    case 13: return (int)dk_settings_fields_check(ENUM_ROW_TOO_WIDE, 1);
    case 14: return (int)dk_settings_fields_check(BAD_KIND, 1);
    default: return 1;
    }
}

int sp_changed_calls(void) { return ch.calls; }
int sp_changed_field(void) { return (int)ch.field; }
int sp_changed_value(void) { return ch.value; }
int sp_puts_at_changed(void) { return ch.puts_at_call; }
int sp_puts(void) { return fs_puts(); }
void sp_put_result(int rc) { fs_put_result(rc); }
int sp_value(int field) { return dk_settings_page_value((uint32_t)field); }
int sp_set(int field, int value) { return (int)dk_settings_page_set((uint32_t)field, value); }

int sp_fills(void) { return ch.fills; }
int sp_fills_at_changed(void) { return ch.fills_at_call; }
