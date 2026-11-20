/* Host test of src/cf/guard.h and guard.c: the allow-list check, the writer classes, the refusals and the counters. The rows are a device file's
 * example (stock [0, 0x180000), runtime = the store [0x200000, 0x300000)); the three return addresses are stand-ins. */
#include <stdio.h>
#include <stdlib.h>
#include "cf/guard.h"

#define STOCK_END 0x180000u
#define STORE_FIRST 0x200000u
#define STORE_END 0x300000u

const uint32_t dkcg_row_n = 2;
const cg_row dkcg_rows[8] = {
    {0, STOCK_END, CG_STOCK},
    {STORE_FIRST, STORE_END, CG_RUNTIME},
};
char dkrt_raw_write_ret[4];
char cg_write_split_ret[4];

int32_t dkcg_write(uint32_t sector, uint32_t bytes, uint32_t ret);
int32_t dkcg_erase(uint32_t sector, uint32_t count, uint32_t ret);
uint32_t dkcg_stats(uint32_t *out, uint32_t cap);

static int failed;
#define CHECK(c)                                                                                   \
    do {                                                                                           \
        if (!(c)) {                                                                                \
            printf("%s:%d: check failed: %s\n", __FILE__, __LINE__, #c);                           \
            failed = 1;                                                                            \
        }                                                                                          \
    } while (0)

static const uint32_t RAW = 0x1000, SPLIT = 0x2000, STOCK_CALLER = 0x3000;

static int32_t w(uint32_t ret, uint32_t sector, uint32_t bytes)
{
    cg_verdict v = {0xffu, 0xffu};
    return cg_check_write(dkcg_rows, dkcg_row_n, RAW, SPLIT, ret, sector, bytes, &v);
}
static int32_t e(uint32_t sector, uint32_t count)
{
    cg_verdict v;
    return cg_check_erase(dkcg_rows, dkcg_row_n, sector, count, &v);
}

static void stock_write_fits_and_refuses_at_each_boundary(void)
{
    CHECK(w(STOCK_CALLER, 0, 512) == 0);             /* the first sector */
    CHECK(w(STOCK_CALLER, STOCK_END - 1, 512) == 0); /* the last sector */
    CHECK(w(STOCK_CALLER, STOCK_END - 1, 513) ==
          CG_E_RANGE); /* one byte into the next sector: two sectors */
    CHECK(w(STOCK_CALLER, STOCK_END, 512) == CG_E_RANGE); /* the first sector past the row */
    CHECK(w(STOCK_CALLER, 0, STOCK_END * 512u) == 0);     /* the whole row */
    CHECK(w(STOCK_CALLER, 0, STOCK_END * 512u + 1u) == CG_E_RANGE); /* one byte more */
    CHECK(w(STOCK_CALLER, STORE_FIRST - 1, 512) == CG_E_RANGE);     /* the guard band */
    CHECK(w(STOCK_CALLER, STORE_FIRST, 512) == CG_E_RANGE);         /* the store first sector */
    CHECK(w(STOCK_CALLER, STORE_END - 1, 512) == CG_E_RANGE);       /* the store last sector */
    CHECK(w(STOCK_CALLER, STORE_END, 512) == CG_E_RANGE);           /* past the store: no row */
    CHECK(w(STOCK_CALLER, 0xffffffffu, 512) == CG_E_RANGE);         /* the last possible sector */
    CHECK(w(STOCK_CALLER, 0xfffffffeu, 0x7fffffffu) == CG_E_RANGE); /* the sum would wrap */
    CHECK(w(STOCK_CALLER, 0x17ffffu, 0x7fffffffu) ==
          CG_E_RANGE); /* a huge count from the last sector */
    CHECK(w(STOCK_CALLER, 0, 0x7fffffffu) ==
          CG_E_RANGE); /* a count that spans every row and the gaps */
}

static void runtime_class_holds_only_the_store(void)
{
    CHECK(w(RAW, STORE_FIRST, 512) == 0);
    CHECK(w(RAW, STORE_END - 1, 512) == 0);
    CHECK(w(RAW, STORE_FIRST, (STORE_END - STORE_FIRST) * 512u) == 0); /* the whole store */
    CHECK(w(RAW, STORE_FIRST - 1, 512) == CG_E_RANGE);  /* one sector below the store */
    CHECK(w(RAW, STORE_FIRST - 1, 1024) == CG_E_RANGE); /* straddling the store start */
    CHECK(w(RAW, STORE_END - 1, 1024) == CG_E_RANGE);   /* straddling the store end */
    CHECK(w(RAW, STORE_END, 512) == CG_E_RANGE);
    CHECK(w(RAW, 0, 512) == CG_E_RANGE); /* the runtime may not write the stock area */
    CHECK(w(RAW, STOCK_END - 1, 512) == CG_E_RANGE);
    CHECK(w(STOCK_CALLER, STORE_FIRST, 512) ==
          CG_E_RANGE); /* and a stock caller may not write the store */
}

static void split_negative_and_zero_length_go_to_the_stock_code(void)
{
    CHECK(w(SPLIT, STORE_FIRST, 512) ==
          0); /* the nested 64 KiB piece of a request that was checked as a whole */
    CHECK(w(SPLIT, 0xffffffffu, 0x10000u) == 0);
    CHECK(w(STOCK_CALLER, STORE_FIRST, 0) ==
          0); /* zero length: the stock code returns 0 without a command (the console command) */
    CHECK(w(STOCK_CALLER, 0xffffffffu, 0) == 0);
    CHECK(w(STOCK_CALLER, STORE_FIRST, 0xfffffe00u) ==
          0); /* negative as i32: the stock code answers -12 */
    CHECK(w(STOCK_CALLER, STORE_FIRST, 0x80000000u) == 0);
}

static void bytes_round_up_to_sectors(void)
{
    CHECK(cg_sectors_of(1) == 1 && cg_sectors_of(511) == 1 && cg_sectors_of(512) == 1 &&
          cg_sectors_of(513) == 2);
    CHECK(cg_sectors_of(0x7fffffffu) == 0x400000u);
    CHECK(cg_sectors_of(0xffffffffu) == 0x800000u);
    CHECK(w(STOCK_CALLER, 0, 100) == 0);
    CHECK(w(STOCK_CALLER, STOCK_END - 1, 100) == 0);
    CHECK(w(STOCK_CALLER, STOCK_END, 100) == CG_E_RANGE);
}

static void erase_is_stock_class_and_never_empty(void)
{
    CHECK(e(0, 1) == 0);
    CHECK(e(0x2000, 0x800) == 0);     /* an erase-all piece */
    CHECK(e(0x80000, 0x100000) == 0); /* FORMAT of the projects, 0x80000..0x17ffff */
    CHECK(e(STOCK_END - 1, 1) == 0);
    CHECK(e(STOCK_END - 1, 2) == CG_E_RANGE);
    CHECK(e(STOCK_END, 1) == CG_E_RANGE);
    CHECK(e(STORE_FIRST, 1) == CG_E_RANGE); /* the store: a stock erase */
    CHECK(e(STORE_FIRST - 1, 2) == CG_E_RANGE);
    CHECK(e(0, 0) == CG_E_RANGE); /* count 0: the end would be sector - 1 */
    CHECK(e(STORE_FIRST, 0) == CG_E_RANGE);
    CHECK(e(0xffffffffu, 1) == CG_E_RANGE);
    CHECK(e(1, 0xffffffffu) == CG_E_RANGE);
}

static void rows_of_the_wrong_class_or_count_are_ignored(void)
{
    const cg_row rows[3] = {{0, 100, CG_STOCK}, {200, 300, CG_STOCK}, {100, 200, CG_RUNTIME}};
    CHECK(cg_allowed(rows, 3, CG_STOCK, 99, 1));
    CHECK(!cg_allowed(rows, 3, CG_STOCK, 99, 2)); /* a range does not span two rows */
    CHECK(!cg_allowed(rows, 3, CG_STOCK, 150, 1));
    CHECK(cg_allowed(rows, 3, CG_STOCK, 250, 50));
    CHECK(!cg_allowed(rows, 2, CG_RUNTIME, 150, 1)); /* n counts the rows */
    CHECK(cg_allowed(rows, 3, CG_RUNTIME, 150, 1));
    CHECK(!cg_allowed(rows, 0, CG_STOCK, 0, 1)); /* no rows: nothing allowed */
}

static void counters_and_last_request_follow_the_refusals(void)
{
    uint32_t o[8];
    int32_t r;
    CHECK(dkcg_stats(o, 5) == 0); /* too small: nothing written */
    CHECK(dkcg_stats(o, 8) == 6 && o[0] == 1 && o[1] == 0 && o[2] == 0 && o[3] == 0 && o[4] == 0 &&
          o[5] == 0);
    CHECK(dkcg_write(0, 512, STOCK_CALLER) == 0);
    CHECK(dkcg_erase(0, 1, STOCK_CALLER) == 0);
    CHECK(dkcg_stats(o, 8) == 6 && o[1] == 0 && o[2] == 0); /* the allow path stores nothing */
    r = dkcg_write(STORE_FIRST, 1024, STOCK_CALLER);
    CHECK(r == CG_E_RANGE);
    CHECK(dkcg_stats(o, 8) == 6 && o[1] == 1 && o[2] == 0 && o[3] == STORE_FIRST && o[4] == 2 &&
          o[5] == CG_STOCK);
    r = dkcg_write(STOCK_END, 513, (uint32_t)(uintptr_t)dkrt_raw_write_ret);
    CHECK(r == CG_E_RANGE);
    CHECK(dkcg_stats(o, 8) == 6 && o[1] == 2 && o[3] == STOCK_END && o[4] == 2 &&
          o[5] == CG_RUNTIME);
    r = dkcg_write(STORE_FIRST, 512, (uint32_t)(uintptr_t)cg_write_split_ret);
    CHECK(r == 0);
    r = dkcg_erase(STORE_FIRST, 3, STOCK_CALLER);
    CHECK(r == CG_E_RANGE);
    CHECK(dkcg_stats(o, 8) == 6 && o[1] == 2 && o[2] == 1 && o[3] == STORE_FIRST && o[4] == 3 &&
          o[5] == (CG_STOCK | (1u << 8)));
    CHECK(dkcg_write((uint32_t)0xffffffffu, 0, STOCK_CALLER) == 0);
    CHECK(dkcg_stats(o, 8) == 6 && o[1] == 2 && o[2] == 1);
}

int main(void)
{
    /* the entries compare with the real symbols' addresses; the pure tests use their own stand-ins */
    stock_write_fits_and_refuses_at_each_boundary();
    runtime_class_holds_only_the_store();
    split_negative_and_zero_length_go_to_the_stock_code();
    bytes_round_up_to_sectors();
    erase_is_stock_class_and_never_empty();
    rows_of_the_wrong_class_or_count_are_ignored();
    counters_and_last_request_follow_the_refusals();
    if (failed)
        return 1;
    printf("card-guard host test: ok\n");
    return 0;
}
