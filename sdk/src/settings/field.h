/* The pure part of the settings page (field.c): the checks of a table and the text of a value. No imports, so host tests link it as it is. */
#ifndef DK_SETTINGS_FIELD_H
#define DK_SETTINGS_FIELD_H
#include <stdint.h>
#include <dk/settings/page.h>

/* DK_OK, or DK_E_FORMAT when a field breaks the rules of <dk/settings/page.h>. */
int32_t dk_settings_fields_check(const dk_settings_field *fields, uint32_t count);
/* The smallest and the largest value of a field (a BOOL is 0 .. 1). */
int32_t dk_settings_field_min(const dk_settings_field *f);
int32_t dk_settings_field_max(const dk_settings_field *f);
int32_t dk_settings_field_clamp(const dk_settings_field *f, int32_t value);
/* The decimal text of `value` (at most 4 characters and the NUL) into `out`; returns its length. */
uint32_t dk_settings_field_encode(int32_t value, char *out);
/* The value of stored text of `len` bytes (no NUL needed): the number when it is a decimal integer in range, else the field's default. */
int32_t dk_settings_field_decode(const dk_settings_field *f, const char *text, uint32_t len);
/* The row text of an ENUM field with the option of `value` (at most DK_UI_ROW characters with the NUL). */
void dk_settings_field_row(const dk_settings_field *f, int32_t value, char *out);
#endif
