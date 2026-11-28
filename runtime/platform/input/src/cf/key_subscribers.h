/* core/input: the key subscribers (version 1). Every key event of the panel passes the firmware's key walker once, in the UI task, before any view; the walker of this module
 * (key-walk.s, entered by the hook `cf.input.hook.key_event`) offers the event to the subscribers below in slot order. The words of `input_keys_info` (the image's info table, name
 * "input_keys_info") are the first lines of the enum; the two calls are key_subscribers.c. Both calls and the walk run in the UI task, so the slots need no mask. */
#ifndef KEY_SUBSCRIBERS_H
#define KEY_SUBSCRIBERS_H

#include <stdint.h>

/* A subscriber: `ev` is the firmware's KeyEvent (+0xc the key code, +0x10 the flags, bit 0 set on a press). Nonzero: it took the event, no later subscriber and no view sees it. */
typedef uint32_t (*key_subscriber)(void *vc, const void *ev);

/* The slots of the table: the subscribers that can be set at once. */
#define KEY_SUBSCRIBER_SLOTS 4u

#define KEYS_INFO_MAGIC 0x4b455953u /* 'KEYS' */
#define KEYS_INFO_VERSION 1u

enum keys_info_word {
    KEYS_INFO_W_MAGIC,
    KEYS_INFO_W_VERSION,
    KEYS_INFO_W_SLOTS,       /* KEY_SUBSCRIBER_SLOTS */
    KEYS_INFO_W_SUBSCRIBE,   /* int32_t key_subscribe(key_subscriber fn) */
    KEYS_INFO_W_UNSUBSCRIBE, /* int32_t key_unsubscribe(key_subscriber fn) */
    KEYS_INFO_WORDS
};

#define KEYS_OK 0
#define KEYS_FULL (-1)      /* every slot holds a subscriber */
#define KEYS_DUPLICATE (-2) /* `fn` is subscribed already */
#define KEYS_NONE (-3)      /* `fn` is null, or (unsubscribe) not subscribed */

/* Puts `fn` in the first free slot. Returns the slot (0 .. KEY_SUBSCRIBER_SLOTS - 1), or KEYS_FULL, KEYS_DUPLICATE or KEYS_NONE with nothing changed. */
int32_t key_subscribe(key_subscriber fn);

/* Frees the slot of `fn`. Returns KEYS_OK, or KEYS_NONE when `fn` holds no slot. */
int32_t key_unsubscribe(key_subscriber fn);

/* The walk: 1 when a subscriber took the event (the walker returns 1 to its caller), 0 when none did or none is set (the stock walker runs). Called by key-walk.s. */
uint32_t key_walk(void *vc, void *ev);

#endif
