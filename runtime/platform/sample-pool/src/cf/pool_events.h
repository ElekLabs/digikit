/* The pool events (dkrt_pool_done, dkrt_pool_lost of dkrt_pool.h) as the adapter delivers them: through two pointer slots of the image (`[[slot]]` pool_done and
 * pool_lost of module.toml) that a loaded samples service fills with its routines. A zero slot (no service loaded, or it was taken down) drops the event: no
 * request of a service that is not there can be waiting. The slots are words in the adapter's data; the linker names them (`[object.externs]`).
 *
 * sample_pool_info is the table the service reads to find the calls of dkrt_pool.h (CoreApi::info, the name `sample_pool_info`): words, big endian, magic 'DKSP',
 * the plug-in point version, one reserved word, then the addresses of dkpool_load, dkpool_release, dkpool_check, dkpool_free_frames, dkpool_tick, dkpool_cancel,
 * dkpool_peaks and dkpool_window_step. Each device's pool.c defines it at its end. */
#ifndef DK_POOL_EVENTS_H
#define DK_POOL_EVENTS_H

#include <stdint.h>

/* a slot holds an address: `uintptr_t` is the 4 bytes of the word on the ColdFire, and the width of a pointer on the host harness of the tests */
extern volatile uintptr_t sp_slot_pool_done;
extern volatile uintptr_t sp_slot_pool_lost;

static inline void sp_pool_done(uint32_t req, int32_t result)
{
    uintptr_t f = sp_slot_pool_done;
    if (f != 0)
        ((void (*)(uint32_t, int32_t))f)(req, result);
}

static inline void sp_pool_lost(void)
{
    uintptr_t f = sp_slot_pool_lost;
    if (f != 0)
        ((void (*)(void))f)();
}

#define SP_INFO_MAGIC 0x444b5350u /* 'DKSP' */

#endif
