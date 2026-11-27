/* project_text: the lines of the project screen. Pure logic: no SDK calls, no library calls (the module is run-time code). */
#ifndef PROJECT_TEXT_H
#define PROJECT_TEXT_H
#include <stdint.h>

#define PROJECT_NAME_BYTES 16 /* the firmware's name field: zero padded */
#define PROJECT_STATUS_FILE 1u /* bits of dk_project_status() */
#define PROJECT_STATUS_DIRTY 2u
#define PROJECT_STATUS_DEFERRED 4u
#define PROJECT_STATUS_IGNORED 8u
#define PROJECT_LINE_MAX (4 + PROJECT_NAME_BYTES + 1) /* "NNN " and the name and a terminator */

/* "NNN NAME": NNN is `slot + 1` in three digits (a slot over 998 shows its last three digits), NAME the bytes of `name` up to the first zero, without the
 * trailing spaces. A name with no characters gives "NNN". */
void project_text_title(char out[PROJECT_LINE_MAX], uint32_t slot, const uint8_t name[PROJECT_NAME_BYTES]);

/* "NO PROJECT", for the screen of a device without a current project. */
const char *project_text_none(void);

/* The line of the save state: FILE IGNORED (bit 3), else CARD BUSY - RETRYING (bit 2), else CHANGED - NOT SAVED (bit 1), else SAVED. */
const char *project_text_state(uint32_t status);

/* The line for a project without digikit data: "NO DIGIKIT DATA" when bit 0 is clear, else "". */
const char *project_text_data(uint32_t status);

#endif
