/* The pure part of the settings page: see field.h. */
#include "field.h"
#include <dk/platform/ui_text.h>

static const uint32_t ROW = DK_UI_ROW; /* characters of a row with its NUL */
#define VALUE_PX 60                      /* the pixel column of the value of a toggle or a value row in a wide page */

static uint32_t text_len(const char *s)
{
    uint32_t n = 0;
    while (s[n])
        n++;
    return n;
}

static int key_ok(const char *k)
{
    uint32_t n = text_len(k);
    if (n == 0 || n > DK_SETTINGS_KEY)
        return 0;
    for (uint32_t i = 0; i < n; i++) {
        if (k[i] <= ' ' || k[i] > '~' || k[i] == '/')
            return 0;
        if (k[i] == '.' && i + 1 < n && k[i + 1] == '.')
            return 0;
    }
    return 1;
}

int32_t dk_settings_field_min(const dk_settings_field *f)
{
    return f->kind == DK_SETTINGS_INT ? f->min : 0;
}

int32_t dk_settings_field_max(const dk_settings_field *f)
{
    return f->kind == DK_SETTINGS_BOOL ? 1 : f->max;
}

int32_t dk_settings_field_clamp(const dk_settings_field *f, int32_t value)
{
    int32_t lo = dk_settings_field_min(f), hi = dk_settings_field_max(f);
    return value < lo ? lo : value > hi ? hi : value;
}

static int field_ok(const dk_settings_field *f)
{
    if (!f->key || !key_ok(f->key) || !f->label || !f->label[0])
        return 0;
    uint32_t lab = text_len(f->label);
    switch (f->kind) {
    case DK_SETTINGS_BOOL:
        return f->def == 0 || f->def == 1 ? lab < ROW : 0;
    case DK_SETTINGS_INT:
        if (f->min < -99 || f->max > 999 || f->min > f->max || f->def < f->min || f->def > f->max)
            return 0;
        return lab < ROW;
    case DK_SETTINGS_ENUM:
        if (f->min != 0 || f->max < 1 || f->max > 31 || f->def < 0 || f->def > f->max || !f->labels)
            return 0;
        for (int32_t i = 0; i <= f->max; i++)
            if (!f->labels[i] || !f->labels[i][0] || lab + 1 + text_len(f->labels[i]) >= ROW)
                return 0;
        return 1;
    default:
        return 0;
    }
}

int32_t dk_settings_fields_check(const dk_settings_field *fields, uint32_t count)
{
    if (!fields || count == 0 || count > DK_SETTINGS_FIELDS)
        return DK_E_FORMAT;
    for (uint32_t i = 0; i < count; i++) {
        if (!field_ok(&fields[i]))
            return DK_E_FORMAT;
        for (uint32_t j = 0; j < i; j++) {
            const char *a = fields[i].key, *b = fields[j].key;
            while (*a && *a == *b)
                a++, b++;
            if (*a == *b)
                return DK_E_FORMAT; /* two fields with one key */
        }
    }
    return DK_OK;
}

uint32_t dk_settings_field_encode(int32_t value, char *out)
{
    uint32_t n = 0;
    uint32_t u = (uint32_t)value;
    char digits[4];
    uint32_t d = 0;
    if (value < 0) {
        out[n++] = '-';
        u = (uint32_t)(-value);
    }
    do {
        digits[d++] = (char)('0' + u % 10);
        u /= 10;
    } while (u && d < 4);
    while (d)
        out[n++] = digits[--d];
    out[n] = 0;
    return n;
}

int32_t dk_settings_field_decode(const dk_settings_field *f, const char *text, uint32_t len)
{
    uint32_t i = 0;
    int neg = 0;
    int32_t v = 0;
    if (len && text[0] == '-') {
        neg = 1;
        i = 1;
    }
    if (i == len || len - i > 3)
        return f->def;
    for (; i < len; i++) {
        if (text[i] < '0' || text[i] > '9')
            return f->def;
        v = v * 10 + (text[i] - '0');
    }
    if (neg)
        v = -v;
    return v < dk_settings_field_min(f) || v > dk_settings_field_max(f) ? f->def : v;
}

void dk_settings_field_row(const dk_settings_field *f, int32_t value, char *out)
{
    uint32_t n = 0, o = 0;
    const char *opt = f->labels[dk_settings_field_clamp(f, value)];
    while (opt[o])
        o++;
    for (const char *s = f->label; *s && n + o + 1 < ROW - 1; s++)
        out[n++] = *s;
    out[n] = 0;
    /* the option starts where the value of a toggle or a value row does; the padding stops when the option would not fit */
    uint32_t label = n;
    n = (uint32_t)dk_ui_pad_px(out, (int)n, (int)(ROW - o), VALUE_PX);
    if (n == label)
        out[n++] = ' ';
    for (const char *s = opt; *s && n < ROW - 1; s++)
        out[n++] = *s;
    out[n] = 0;
}
