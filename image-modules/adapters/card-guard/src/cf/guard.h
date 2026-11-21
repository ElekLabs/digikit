/* core/card-guard: the allow-list check, plain logic with no firmware access (host tests: tests/guard-host.c).
 *
 * A request to the firmware's eMMC block write or erase is described by its first sector and its length in sectors. The allow-list is a few rows
 * `{ first, end, who }` (sectors, `end` exclusive; the device file's [card.guard], laid out by src/cf/<dev>/device.s). A request is allowed only when the
 * WHOLE range lies inside one row whose class is the caller's. Two classes:
 *   CG_RUNTIME  the digikit runtime's raw card write: the one caller whose return address is `raw_ret` (a label in the runtime's trampoline).
 *   CG_STOCK    every other caller of the write, and every caller of the erase (the runtime does not erase).
 * The class comes from the return address, not from a flag: the RTOS is preemptive, so a "the runtime is writing" flag would let a stock write through
 * while it is set, and a return address cannot be forged by another task.
 * No 64-bit arithmetic: a row test is `first >= row.first && first < row.end && sectors <= row.end - first`, which cannot wrap. */
#ifndef CG_GUARD_H
#define CG_GUARD_H

#include <stdint.h>

enum { CG_STOCK = 0, CG_RUNTIME = 1 };

/* The error the stock code gives for "sector out of range" (the card driver's -10): every caller of the block write and erase tests the result for non-zero or
 * negative and stops with its own error. */
#define CG_E_RANGE (-10)

/* One row as device.s lays it out: three words. */
typedef struct {
    uint32_t first, end, who;
} cg_row;

/* What a refusal records. */
typedef struct {
    uint32_t who;
    uint32_t sectors;
} cg_verdict;

static inline uint32_t cg_sectors_of(uint32_t bytes)
{
    return (bytes >> 9) + ((bytes & 511u) != 0u); /* ceil(bytes / 512), no overflow */
}

/* Does one row of the class `who` hold [first, first + sectors)? */
static inline int cg_allowed(const cg_row *rows, uint32_t n, uint32_t who, uint32_t first,
                             uint32_t sectors)
{
    uint32_t i;
    for (i = 0; i < n; i++) {
        const cg_row *r = &rows[i];
        if (r->who == who && first >= r->first && first < r->end && sectors <= r->end - first)
            return 1;
    }
    return 0;
}

/* The block write `f(sector, bytes, buf)`. Returns 0 to let the stock code run (it also decides what it does with a negative or zero length, and the
 * nested 64 KiB pieces of a larger write are parts of a request that was checked as a whole), or CG_E_RANGE; a refusal fills `v`.
 *   ret         the return address of the call (0(SP) at the entry)
 *   raw_ret     the runtime's raw-write return label; ret == raw_ret is class CG_RUNTIME
 *   split_ret   the return address of the stock function's own recursive call */
static inline int32_t cg_check_write(const cg_row *rows, uint32_t n, uint32_t raw_ret,
                                     uint32_t split_ret, uint32_t ret, uint32_t sector,
                                     uint32_t bytes, cg_verdict *v)
{
    uint32_t who, sectors;
    if ((int32_t)bytes <= 0)
        return 0; /* negative: the stock code answers -12; zero: it answers 0 without a card command */
    if (ret == split_ret)
        return 0;
    who = ret == raw_ret ? CG_RUNTIME : CG_STOCK;
    sectors = cg_sectors_of(bytes);
    if (cg_allowed(rows, n, who, sector, sectors))
        return 0;
    v->who = who;
    v->sectors = sectors;
    return CG_E_RANGE;
}

/* The block erase `f(sector, count)`: CMD35 sector, CMD36 sector + count - 1. A count of 0 would make the end sector - 1 and no stock caller passes it, so it is
 * refused like any other request that no row holds. */
static inline int32_t cg_check_erase(const cg_row *rows, uint32_t n, uint32_t sector,
                                     uint32_t count, cg_verdict *v)
{
    if (count != 0u && cg_allowed(rows, n, CG_STOCK, sector, count))
        return 0;
    v->who = CG_STOCK;
    v->sectors = count;
    return CG_E_RANGE;
}

#endif
