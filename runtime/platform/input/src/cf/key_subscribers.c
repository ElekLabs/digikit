/* core/input: the table of key subscribers. The slots are private to this file: callers subscribe and unsubscribe, the walker of key-walk.s calls key_walk. */
#include <stdint.h>
#include "cf/key_subscribers.h"

/* the code is placed as it is: no jump tables (data in the code) */
#ifdef __m68k__
#pragma GCC optimize("no-jump-tables")
#endif

/* an empty slot is null; the file's .bss starts all empty */
static key_subscriber slots[KEY_SUBSCRIBER_SLOTS];

/* the data table of this module (the image's info table, name "input_keys_info"): the addresses are link-time constants, so the words are pointer-sized */
const uintptr_t input_keys_info[KEYS_INFO_WORDS] = {
    KEYS_INFO_MAGIC,
    KEYS_INFO_VERSION,
    KEY_SUBSCRIBER_SLOTS,
    (uintptr_t)key_subscribe,
    (uintptr_t)key_unsubscribe,
};

int32_t key_subscribe(key_subscriber fn)
{
    if (!fn)
        return KEYS_NONE;
    int32_t free_slot = KEYS_FULL;
    for (uint32_t i = 0; i < KEY_SUBSCRIBER_SLOTS; i++) {
        if (slots[i] == fn)
            return KEYS_DUPLICATE;
        if (!slots[i] && free_slot == KEYS_FULL)
            free_slot = (int32_t)i;
    }
    if (free_slot != KEYS_FULL)
        slots[free_slot] = fn;
    return free_slot;
}

int32_t key_unsubscribe(key_subscriber fn)
{
    if (!fn)
        return KEYS_NONE;
    for (uint32_t i = 0; i < KEY_SUBSCRIBER_SLOTS; i++) {
        if (slots[i] == fn) {
            slots[i] = 0;
            return KEYS_OK;
        }
    }
    return KEYS_NONE;
}

uint32_t key_walk(void *vc, void *ev)
{
    for (uint32_t i = 0; i < KEY_SUBSCRIBER_SLOTS; i++) {
        key_subscriber fn = slots[i];
        if (fn && fn(vc, ev))
            return 1;
    }
    return 0;
}
