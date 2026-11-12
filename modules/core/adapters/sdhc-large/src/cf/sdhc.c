/* core/sdhc-large: the calls of the module (sdk/sdk.toml: sdhc_large_range, sdhc_large_ctl). The limit logic and its data are in sdhc.s. */

/* the code is placed as it is: no jump tables (data in the code) */
#pragma GCC optimize("no-jump-tables")

typedef unsigned int u32;
typedef unsigned char u8;

extern volatile u8 sl_off; /* sdhc.s: tst.b reads the first byte; 0 = on */
extern volatile u32 sl_lo, sl_hi;
extern volatile u32 sl_cnt[4];

/* The range is closed (hi = 0) while it changes, so a transfer of another task never sees half of it. */
void sdhc_large_range(u32 lo, u32 hi)
{
    sl_hi = 0;
    sl_lo = lo;
    sl_hi = hi;
}

u32 sdhc_large_ctl(u32 key, u32 val)
{
    if (key == 0u) {
        u32 old = sl_off ? 0u : 1u;
        if (val < 2u)
            sl_off = val ? 0u : 1u;
        return old;
    }
    if (key == 1u && val < 4u)
        return sl_cnt[val];
    return 0;
}
