/* Host test of src/cf/lfo-table.c over fake mask functions: what lfo_table_set and lfo_table_clear write, in which order of the mask, and what they refuse.
 * The assembly side (stubs, mask, info words) is tested on the ColdFire core in packages/patchkit/tests/lfo_table.rs. */
#include <stdio.h>
#include <string.h>
#include "cf/lfo-table.h"

#define WAVES 8u
#define FIRST 7u

const uint32_t lfo_table_info[LFO_INFO_WORDS] = {
    LFO_INFO_MAGIC, LFO_INFO_VERSION, FIRST, WAVES, LFO_SAMPLES, 0, 0};
const uint32_t lfo_wave_fn[WAVES] = {0xa000, 0xa010, 0xa020, 0xa030,
                                     0xa040, 0xa050, 0xa060, 0xa070};
lfo_cell lfo_wave_cells[WAVES * LFO_SAMPLES];
uint32_t lfo_fn_table[7 + WAVES];

/* the mask: the fake keeps the depth, and snapshots the tables when it is taken and when it is given back, so the test sees which writes happened inside */
static int depth, masks, restores;
static uint32_t at_mask_fn[7 + WAVES], at_restore_fn[7 + WAVES];
static lfo_cell at_mask_cells[WAVES * LFO_SAMPLES], at_restore_cells[WAVES * LFO_SAMPLES];

uint32_t lfo_irq_mask(void)
{
    depth++;
    masks++;
    memcpy(at_mask_fn, lfo_fn_table, sizeof lfo_fn_table);
    memcpy(at_mask_cells, lfo_wave_cells, sizeof lfo_wave_cells);
    return 0x2000u + (uint32_t)masks;
}

void lfo_irq_restore(uint32_t sr)
{
    depth--;
    restores++;
    memcpy(at_restore_fn, lfo_fn_table, sizeof lfo_fn_table);
    memcpy(at_restore_cells, lfo_wave_cells, sizeof lfo_wave_cells);
    if (sr != 0x2000u + (uint32_t)masks)
        printf("restore got %#x, not the value the mask returned\n", sr);
}

static int failed;
#define CHECK(c)                                                                                   \
    do {                                                                                           \
        if (!(c)) {                                                                                \
            printf("%s:%d: check failed: %s\n", __FILE__, __LINE__, #c);                           \
            failed = 1;                                                                            \
        }                                                                                          \
    } while (0)

static void reset(void)
{
    for (uint32_t i = 0; i < 7 + WAVES; i++)
        lfo_fn_table[i] =
            i < 7 ? 0x4000u + 0x10u * i : 0x4010u; /* the stock entries, then the sine's (slot 1) */
    memset(lfo_wave_cells, 0, sizeof lfo_wave_cells);
    depth = masks = restores = 0;
}

static int16_t s[LFO_SAMPLES];
static void fill(void)
{
    for (int i = 0; i < LFO_SAMPLES; i++)
        s[i] = (int16_t)(i * 2000 - 30000);
    s[0] = 32767;
    s[1] = -32768; /* the largest step */
    s[31] = -32768;
}

int main(void)
{
    fill();

    /* set: the cells of the wave, its entry, one mask held around both, nothing else touched */
    reset();
    CHECK(lfo_table_set(9, s) == 0);
    CHECK(masks == 1 && restores == 1 && depth == 0);
    const uint32_t k = 9 - FIRST;
    for (int i = 0; i < LFO_SAMPLES; i++) {
        int32_t a = s[i], b = s[(i + 1) & 31];
        CHECK(lfo_wave_cells[k * LFO_SAMPLES + i].v == (int32_t)((uint32_t)a << 16));
        CHECK(lfo_wave_cells[k * LFO_SAMPLES + i].d == b - a);
    }
    CHECK(lfo_fn_table[9] == lfo_wave_fn[k]);
    for (uint32_t i = 0; i < 7 + WAVES; i++)
        if (i != 9)
            CHECK(lfo_fn_table[i] == (i < 7 ? 0x4000u + 0x10u * i : 0x4010u));
    /* every write is inside the mask: nothing changed before it was taken, and nothing after it was given back */
    CHECK(at_mask_fn[9] == 0x4010u);
    CHECK(at_mask_cells[k * LFO_SAMPLES].v == 0 && at_mask_cells[k * LFO_SAMPLES].d == 0);
    CHECK(memcmp(at_restore_fn, lfo_fn_table, sizeof lfo_fn_table) == 0);
    CHECK(memcmp(at_restore_cells, lfo_wave_cells, sizeof lfo_wave_cells) == 0);
    CHECK(at_restore_fn[9] == lfo_wave_fn[k]);
    /* the cells of the other waves are not touched */
    for (uint32_t i = 0; i < WAVES * LFO_SAMPLES; i++)
        if (i / LFO_SAMPLES != k)
            CHECK(lfo_wave_cells[i].v == 0 && lfo_wave_cells[i].d == 0);

    /* set again with other samples: the cells change, the entry stays */
    int16_t t[LFO_SAMPLES];
    for (int i = 0; i < LFO_SAMPLES; i++)
        t[i] = (int16_t)(100 * i);
    CHECK(lfo_table_set(9, t) == 0);
    CHECK(lfo_wave_cells[k * LFO_SAMPLES + 5].v == (int32_t)(500u << 16));
    CHECK(lfo_wave_cells[k * LFO_SAMPLES + 31].d == 0 - 3100);
    CHECK(lfo_fn_table[9] == lfo_wave_fn[k]);

    /* the first and the last wave */
    CHECK(lfo_table_set(FIRST, s) == 0 && lfo_fn_table[FIRST] == lfo_wave_fn[0]);
    CHECK(lfo_table_set(FIRST + WAVES - 1, s) == 0 &&
          lfo_fn_table[FIRST + WAVES - 1] == lfo_wave_fn[WAVES - 1]);

    /* clear: the entry is the sine's, in one mask; the cells stay (no reader reaches them) */
    masks = restores = 0;
    lfo_table_clear(9);
    CHECK(masks == 1 && restores == 1 && depth == 0);
    CHECK(lfo_fn_table[9] == lfo_fn_table[LFO_SINE]);
    CHECK(at_mask_fn[9] == lfo_wave_fn[k] && at_restore_fn[9] == lfo_fn_table[LFO_SINE]);
    CHECK(lfo_fn_table[FIRST] == lfo_wave_fn[0]);

    /* outside the range: refused, no mask, no write */
    uint32_t outside[] = {0, 1, 6, FIRST + WAVES, FIRST + WAVES + 1, 127, 0xffffffffu};
    for (unsigned i = 0; i < sizeof outside / sizeof outside[0]; i++) {
        uint32_t fn[7 + WAVES];
        lfo_cell cells[WAVES * LFO_SAMPLES];
        memcpy(fn, lfo_fn_table, sizeof fn);
        memcpy(cells, lfo_wave_cells, sizeof cells);
        masks = restores = 0;
        CHECK(lfo_table_set(outside[i], s) == -1);
        lfo_table_clear(outside[i]);
        CHECK(masks == 0 && restores == 0);
        CHECK(memcmp(fn, lfo_fn_table, sizeof fn) == 0 &&
              memcmp(cells, lfo_wave_cells, sizeof cells) == 0);
    }

    if (failed)
        return 1;
    printf("ok\n");
    return 0;
}
