/* Host driver for the pure text logic of the project manager: project_text. Aborts on the first failed check. */
#undef NDEBUG
#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "../src/c/project_text.h"

static void title_of(uint32_t slot, const char *name, size_t bytes, char out[PROJECT_LINE_MAX])
{
    uint8_t field[PROJECT_NAME_BYTES] = {0};
    memcpy(field, name, bytes);
    project_text_title(out, slot, field);
}

static void title_numbers(void)
{
    char t[PROJECT_LINE_MAX];
    title_of(0, "A", 1, t);
    assert(strcmp(t, "001 A") == 0);
    title_of(11, "DUB", 3, t);
    assert(strcmp(t, "012 DUB") == 0);
    title_of(127, "X", 1, t);
    assert(strcmp(t, "128 X") == 0);
}

static void title_names(void)
{
    char t[PROJECT_LINE_MAX];
    title_of(2, "INTRO   ", 8, t); /* trailing spaces go */
    assert(strcmp(t, "003 INTRO") == 0);
    title_of(2, "MY  SET", 7, t); /* inner spaces stay */
    assert(strcmp(t, "003 MY  SET") == 0);
    title_of(2, "", 0, t); /* no name: the number only */
    assert(strcmp(t, "003") == 0);
    title_of(2, "   ", 3, t);
    assert(strcmp(t, "003") == 0);
    title_of(2, "A\0Z", 3, t); /* the name ends at the first zero */
    assert(strcmp(t, "003 A") == 0);
    uint8_t full[PROJECT_NAME_BYTES];
    memset(full, 'Q', sizeof full); /* a field with no zero: all 16 bytes, then the terminator */
    project_text_title(t, 0, full);
    assert(strlen(t) == 4 + PROJECT_NAME_BYTES);
}

static void state_lines(void)
{
    assert(strcmp(project_text_state(0), "SAVED") == 0);
    assert(strcmp(project_text_state(PROJECT_STATUS_FILE), "SAVED") == 0);
    assert(strcmp(project_text_state(PROJECT_STATUS_DIRTY), "CHANGED - NOT SAVED") == 0);
    assert(strcmp(project_text_state(PROJECT_STATUS_DEFERRED), "CARD BUSY - RETRYING") == 0);
    assert(strcmp(project_text_state(PROJECT_STATUS_IGNORED), "FILE IGNORED") == 0);
    /* the order of the bits: ignored, then busy, then changed */
    assert(strcmp(project_text_state(PROJECT_STATUS_DIRTY | PROJECT_STATUS_DEFERRED),
                  "CARD BUSY - RETRYING") == 0);
    assert(strcmp(project_text_state(7u | PROJECT_STATUS_IGNORED), "FILE IGNORED") == 0);
}

static void data_line(void)
{
    assert(strcmp(project_text_data(0), "NO DIGIKIT DATA") == 0);
    assert(strcmp(project_text_data(PROJECT_STATUS_DIRTY), "NO DIGIKIT DATA") == 0);
    assert(strcmp(project_text_data(PROJECT_STATUS_FILE), "") == 0);
    assert(strcmp(project_text_none(), "NO PROJECT") == 0);
}

int main(void)
{
    title_numbers();
    title_names();
    state_lines();
    data_line();
    return 0;
}
