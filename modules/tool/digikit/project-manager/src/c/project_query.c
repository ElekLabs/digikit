/* project_query: see project_query.h. */
#include "project_query.h"

#include <dk/core/status.h>

uint32_t project_query_text(char *out, uint32_t capacity, const uint8_t *bytes, uint32_t len)
{
    uint32_t n = 0;
    if (capacity == 0)
        return 0;
    while (n < len && n < capacity - 1 && bytes[n])
        n++;
    while (n && bytes[n - 1] == ' ')
        n--;
    for (uint32_t i = 0; i < n; i++)
        out[i] = (bytes[i] >= 0x20 && bytes[i] <= 0x7e) ? (char)bytes[i] : '?';
    out[n] = 0;
    return n;
}

void project_query_flags(uint8_t flags, uint8_t *current, uint8_t *has_data, uint8_t *data_unknown)
{
    *current = (flags & PROJECT_QUERY_CURRENT) != 0;
    *has_data = (flags & PROJECT_QUERY_DATA) != 0;
    *data_unknown = (flags & PROJECT_QUERY_DATA_UNKNOWN) != 0;
}

uint32_t project_query_next(uint32_t count, uint32_t max, uint32_t last_slot)
{
    if (count == 0 || count < max || last_slot >= PROJECT_QUERY_LAST_SLOT)
        return 0;
    return last_slot + 1;
}

int32_t project_query_get_outcome(int32_t result, uint8_t *loading, uint8_t *found)
{
    *loading = 0;
    *found = 0;
    if (result == 0) {
        *found = 1;
        return 0;
    }
    if (result == DK_E_AGAIN) {
        *loading = 1;
        return 0;
    }
    if (result == DK_E_NOT_FOUND)
        return 0;
    return result;
}

int32_t project_query_settings_outcome(int32_t result, uint8_t *loading, uint8_t *changed,
                                       uint8_t *found)
{
    *loading = 0;
    *changed = 0;
    *found = 0;
    if (result >= 0) {
        *found = 1;
        return 0;
    }
    if (result == DK_E_AGAIN) {
        *loading = 1;
        return 0;
    }
    if (result == DK_E_CHANGED) {
        *changed = 1;
        return 0;
    }
    if (result == DK_E_NOT_FOUND)
        return 0;
    return result;
}

uint32_t project_query_value(uint8_t *out, uint32_t capacity, const uint8_t *head,
                             uint32_t head_len)
{
    uint32_t n = head_len < capacity ? head_len : capacity;
    for (uint32_t i = 0; i < n; i++)
        out[i] = head[i];
    return n;
}
