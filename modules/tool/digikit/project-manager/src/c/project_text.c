/* project_text: see project_text.h. */
#include "project_text.h"

void project_text_title(char out[PROJECT_LINE_MAX], uint32_t slot, const uint8_t name[PROJECT_NAME_BYTES])
{
    uint32_t n = slot + 1, hundreds = 0, tens = 0;
    while (n >= 1000)
        n -= 1000;
    while (n >= 100) {
        n -= 100;
        hundreds++;
    }
    while (n >= 10) {
        n -= 10;
        tens++;
    }
    out[0] = (char)('0' + hundreds);
    out[1] = (char)('0' + tens);
    out[2] = (char)('0' + n);
    uint32_t len = 0;
    while (len < PROJECT_NAME_BYTES && name[len])
        len++;
    while (len && name[len - 1] == ' ')
        len--;
    uint32_t at = 3;
    if (len)
        out[at++] = ' ';
    for (uint32_t i = 0; i < len; i++)
        out[at++] = (char)name[i];
    out[at] = 0;
}

const char *project_text_none(void)
{
    return "NO PROJECT";
}

const char *project_text_state(uint32_t status)
{
    if (status & PROJECT_STATUS_IGNORED)
        return "FILE IGNORED";
    if (status & PROJECT_STATUS_DEFERRED)
        return "CARD BUSY - RETRYING";
    if (status & PROJECT_STATUS_DIRTY)
        return "CHANGED - NOT SAVED";
    return "SAVED";
}

const char *project_text_data(uint32_t status)
{
    return (status & PROJECT_STATUS_FILE) ? "" : "NO DIGIKIT DATA";
}
