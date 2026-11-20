/* core/card-guard: the two functions the hook shims call (hooks.s) and the counters. The decision is guard.h; the rows are device.s (the device file's [card.guard]).
 * Not called in a refusal-free path with more than the check: the allow path stores nothing. A refusal raises a counter and records the request; the runtime
 * reads them with `dkcg_stats` from its own tick and raises the log event (this code runs on the stack of a stock task, so it makes no call that can block). */
#include <stdint.h>
#include "cf/guard.h"

/* the code is placed as it is: no jump tables (data in the code) */
#ifdef __m68k__
#pragma GCC optimize("no-jump-tables")
#endif

extern const uint32_t dkcg_row_n;
extern const cg_row dkcg_rows[]; /* device.s */
extern char dkrt_raw_write_ret
    []; /* core/runtime tramp.s: the label after the `jsr` of its raw block write */
extern char cg_write_split_ret[]; /* anchor cf.storage.site.block_write.split_ret */

/* [0] refused writes, [1] refused erases, [2] sector, [3] sectors, [4] class and kind of the last refusal (class | kind << 8; kind 0 write, 1 erase).
 * A racing refusal of two tasks may lose a count or mix two `last` fields: both are diagnostics, and a refusal is already an error. */
static volatile uint32_t st[5];

#define STATS_VERSION 1u
#define STATS_WORDS 6u

static void note(uint32_t kind, uint32_t sector, const cg_verdict *v)
{
    st[2] = sector;
    st[3] = v->sectors;
    st[4] = v->who | (kind << 8);
    st[kind]++;
}

/* The block write's hook (hooks.s): 0 = continue in the stock code, else the error to return to the caller. */
int32_t dkcg_write(uint32_t sector, uint32_t bytes, uint32_t ret)
{
    cg_verdict v;
    int32_t e = cg_check_write(dkcg_rows, dkcg_row_n, (uint32_t)(uintptr_t)dkrt_raw_write_ret,
                               (uint32_t)(uintptr_t)cg_write_split_ret, ret, sector, bytes, &v);
    if (e)
        note(0, sector, &v);
    return e;
}

int32_t dkcg_erase(uint32_t sector, uint32_t count, uint32_t ret)
{
    cg_verdict v;
    int32_t e;
    (void)ret;
    e = cg_check_erase(dkcg_rows, dkcg_row_n, sector, count, &v);
    if (e)
        note(1, sector, &v);
    return e;
}

/* The counters for the runtime (INFO, HEALTH, the log event): out[0] = 1 (version), then refused writes, refused erases, the sector, the length in sectors and
 * class | kind << 8 of the last refusal. `cap` is the room of `out` in words; returns the number of words written. */
uint32_t dkcg_stats(uint32_t *out, uint32_t cap)
{
    uint32_t i;
    if (cap < STATS_WORDS)
        return 0;
    out[0] = STATS_VERSION;
    for (i = 0; i < 5u; i++)
        out[1 + i] = st[i];
    return STATS_WORDS;
}
