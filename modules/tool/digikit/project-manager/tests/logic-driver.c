/* Host driver for the pure text logic of the project manager: project_text. Aborts on the first failed check. */
#undef NDEBUG
#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "../src/c/project_query.h"
#include "../src/c/project_text.h"
#include <dk/core/status.h>

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
    title_of(2, "A\x80Z\x1f", 4, t); /* a byte the font may not have shows as '?' */
    assert(strcmp(t, "003 A?Z?") == 0);
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

static void query_text(void)
{
    char out[PROJECT_QUERY_NAME_MAX + 1];
    uint8_t name[16] = {'D', 'U', 'B', ' ', ' ', 0};
    assert(project_query_text(out, sizeof out, name, 16) ==
           3); /* zero ends it, trailing spaces go */
    assert(strcmp(out, "DUB") == 0);
    uint8_t odd[4] = {'A', 0x80, 0x1f, 'Z'};
    assert(project_query_text(out, sizeof out, odd, 4) ==
           4); /* not printable: '?', so the text is valid UTF-8 */
    assert(strcmp(out, "A??Z") == 0);
    uint8_t full[16];
    memset(full, 'Q', sizeof full); /* no zero: all 16 bytes fit with the terminator */
    assert(project_query_text(out, sizeof out, full, 16) == 16);
    assert(out[16] == 0);
    char small[4];
    assert(project_query_text(small, sizeof small, full, 16) == 3); /* never past the capacity */
    assert(strcmp(small, "QQQ") == 0);
    assert(project_query_text(out, sizeof out, name, 0) == 0); /* an empty name */
    assert(out[0] == 0);
    uint8_t blank[3] = {' ', ' ', ' '};
    assert(project_query_text(out, sizeof out, blank, 3) == 0);
}

static void query_flags(void)
{
    uint8_t current, data, unknown;
    project_query_flags(0, &current, &data, &unknown);
    assert(!current && !data && !unknown);
    project_query_flags(PROJECT_QUERY_CURRENT | PROJECT_QUERY_DATA, &current, &data, &unknown);
    assert(current == 1 && data == 1 && unknown == 0);
    project_query_flags(PROJECT_QUERY_DATA_UNKNOWN, &current, &data, &unknown);
    assert(current == 0 && data == 0 && unknown == 1);
}

static void query_next(void)
{
    assert(project_query_next(16, 16, 20) == 21); /* a full page: ask after its last slot */
    assert(project_query_next(5, 16, 20) == 0);   /* a short page is the end */
    assert(project_query_next(0, 16, 0) == 0);
    assert(project_query_next(16, 16, 127) == 0); /* the last slot of the device */
}

static void query_get_outcome(void)
{
    uint8_t loading = 9, found = 9;
    assert(project_query_get_outcome(0, &loading, &found) == 0);
    assert(loading == 0 && found == 1);
    assert(project_query_get_outcome(DK_E_AGAIN, &loading, &found) == 0);
    assert(loading == 1 && found == 0);
    assert(project_query_get_outcome(DK_E_NOT_FOUND, &loading, &found) == 0);
    assert(loading == 0 && found == 0);
    assert(project_query_get_outcome(DK_E_RANGE, &loading, &found) ==
           DK_E_RANGE); /* any other code is the handler's refusal */
    assert(loading == 0 && found == 0);
}

int main(void)
{
    title_numbers();
    title_names();
    state_lines();
    data_line();
    query_text();
    query_flags();
    query_next();
    query_get_outcome();
    return 0;
}
