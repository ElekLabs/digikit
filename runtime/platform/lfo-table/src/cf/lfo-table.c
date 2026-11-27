/* core/lfo-table: set and clear of an added LFO wave. The LFO tick and its second reader call the entry of `lfo_fn_table[wave]` (a stock function, or a stub of lfo-wave.inc that reads
 * the cells of its wave); both run in an interrupt or above the UI task, so a change of a wave is made under the interrupt mask, which keeps the tick out. The words of the table
 * (first wave, count) are `lfo_table_info`, written by lfo-wave.inc from the device file. */
#include <stdint.h>
#include "cf/lfo-table.h"

/* the code is placed as it is: no jump tables (data in the code) */
#ifdef __m68k__
#pragma GCC optimize("no-jump-tables")
#endif

extern const uint32_t lfo_table_info[]; /* lfo-wave.inc */
extern const uint32_t lfo_wave_fn[];    /* lfo-wave.inc: the stub of added wave k */
extern lfo_cell lfo_wave_cells[];       /* lfo-wave.inc (.bss): N x 32 cells */
extern uint32_t lfo_fn_table[];         /* the copy of the wave function table (section lfo_fn): 7 stock entries, then N */
extern uint32_t lfo_irq_mask(void);     /* lfo-wave.inc: raises the interrupt level to 7 and returns the old status register */
extern void lfo_irq_restore(uint32_t sr);

int32_t lfo_table_set(uint32_t wave, const int16_t *samples)
{
    uint32_t first = lfo_table_info[LFO_INFO_W_FIRST];
    if (wave < first || wave - first >= lfo_table_info[LFO_INFO_W_WAVES])
        return -1;
    uint32_t k = wave - first;
    lfo_cell *c = lfo_wave_cells + k * LFO_SAMPLES;
    uint32_t sr = lfo_irq_mask();
    for (uint32_t i = 0; i < LFO_SAMPLES; i++) {
        int32_t a = samples[i];
        int32_t b = samples[(i + 1) & (LFO_SAMPLES - 1)];
        c[i].v = (int32_t)((uint32_t)a << 16);
        c[i].d = b - a;
    }
    lfo_fn_table[wave] = lfo_wave_fn[k];
    lfo_irq_restore(sr);
    return 0;
}

void lfo_table_clear(uint32_t wave)
{
    uint32_t first = lfo_table_info[LFO_INFO_W_FIRST];
    if (wave < first || wave - first >= lfo_table_info[LFO_INFO_W_WAVES])
        return;
    uint32_t sr = lfo_irq_mask();
    lfo_fn_table[wave] = lfo_fn_table[LFO_SINE];
    lfo_irq_restore(sr);
}
