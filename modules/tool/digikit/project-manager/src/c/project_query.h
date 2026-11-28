/* project_query: the mapping of the project service's results to the fields of the Workbench queries (workbench/project_manager.proto).
 * Pure logic: no SDK calls, no library calls (the module is run-time code), and no generated types, so a host driver can run it. */
#ifndef PROJECT_QUERY_H
#define PROJECT_QUERY_H
#include <stdint.h>

#define PROJECT_QUERY_NAME_MAX 16 /* the `max_len` of a name field */
#define PROJECT_QUERY_ID_MAX 31   /* the `max_len` of a module id field */
#define PROJECT_QUERY_LAST_SLOT 127

#define PROJECT_QUERY_CURRENT 1u /* the flag bits of dk_project_entry */
#define PROJECT_QUERY_DATA 2u
#define PROJECT_QUERY_DATA_UNKNOWN 4u

/* Copies up to `capacity - 1` of the `len` bytes into `out` as text and terminates it: the text ends at the first zero byte, trailing spaces go, and a
 * byte that is not printable ASCII becomes '?' (the page gets valid UTF-8). Returns the length of the text. */
uint32_t project_query_text(char *out, uint32_t capacity, const uint8_t *bytes, uint32_t len);

/* The three booleans of a listed project from its flags. */
void project_query_flags(uint8_t flags, uint8_t *current, uint8_t *has_data, uint8_t *data_unknown);

/* The slot to ask for the next page: after a full page (`count` equal to `max`) the slot after the last one listed, else 0. 0 also when the last slot
 * of the device was listed. */
uint32_t project_query_next(uint32_t count, uint32_t max, uint32_t last_slot);

/* The outcome of dk_project_get `result` for the reply: 0 or DK_E_AGAIN or DK_E_NOT_FOUND set `loading` and `found` and return 0; any other code
 * is returned for the handler to refuse. */
int32_t project_query_get_outcome(int32_t result, uint8_t *loading, uint8_t *found);

#define PROJECT_QUERY_VALUE_MAX                                                                    \
    64 /* the `max_len` of a value field: the service gives at most this many value bytes */

/* The outcome of dk_project_settings `result` for the reply: a count (0 or more) sets `found`; DK_E_AGAIN sets `loading`; DK_E_CHANGED sets `changed`; DK_E_NOT_FOUND
 * leaves all three clear. Each returns 0; any other code is returned for the handler to refuse. */
int32_t project_query_settings_outcome(int32_t result, uint8_t *loading, uint8_t *changed,
                                       uint8_t *found);

/* Copies `head_len` value bytes (at most `capacity`) into `out`: the length copied. */
uint32_t project_query_value(uint8_t *out, uint32_t capacity, const uint8_t *head,
                             uint32_t head_len);

#endif
