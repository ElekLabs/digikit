/* core/sdhc-large: the calls of the module. `sdhc_large_range` and `sdhc_large_ctl` are the runtime's (modules/core/runtime/include/dkrt_sdhc.h);
   `dk_sdhc_large_open` and `dk_sdhc_large_ctl` are the SDK's (sdk/sdk.toml, <dk/platform/sdhc_large.h>). The limit logic and its data are in sdhc.s. */
#include <stdint.h>

typedef struct dk_module_ctx dk_module_ctx;
typedef struct dk_sdhc_large dk_sdhc_large;

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

/* ---- the service imports of <dk/gen/imports.h> (docs/plans/sdk.md section 5.1) ---- */

static int sl_token; /* the dk_sdhc_large handle: only a token */

int32_t dk_sdhc_large_open(dk_module_ctx *ctx, dk_sdhc_large **out)
{
    if (!ctx || !out)
        return -48; /* DK_E_CONTRACT */
    *out = (dk_sdhc_large *)&sl_token;
    return 0;
}

uint32_t dk_sdhc_large_ctl(dk_sdhc_large *sl, uint32_t key, uint32_t val)
{
    (void)sl;
    return sdhc_large_ctl(key, val);
}
