/* The reverse lookup of the device's key map (`dkrt_dev.key_codes`, from [input.keys] of the device file): the id of a firmware key event to the
   logical key (<dk/gen/keys.h>). Pure, so the host can test it (host/tests/keys.rs); panel_keys.c gives it the device's table. */
#ifndef DK_UI_KEY_MAP_H
#define DK_UI_KEY_MAP_H
#include <stdint.h>
#include <dk/gen/keys.h>

/* `codes[k]` is the panel code of logical key k (0: the device has no such key); `n` entries, entry 0 is DK_KEY_NONE. The key whose code is `code`,
   or DK_KEY_NONE when none has it (also for code 0). The first key wins when two share a code. */
dk_key ui_key_from_codes(const uint32_t *codes, uint32_t n, uint32_t code);

#endif
