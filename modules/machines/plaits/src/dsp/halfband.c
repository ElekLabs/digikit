/* Half-band 2x interpolator (48 -> 96 kHz). Plain C99, float only. */
#include "halfband.h"

/* Exported for host tests. */
const int hb_state_size = (int)sizeof(HalfBand);
const int hb_group_delay = HB_GROUP_DELAY;

void hb_init(HalfBand *s)
{
    int i;
    for (i = 0; i < HB_BUFSZ; i++) {
        s->buf[i] = 0.0f;
    }
    s->pos = 0;
}

void hb_up2(HalfBand *s, const float *in48, float *out96, int n48)
{
    const int mask = HB_BUFSZ - 1;
    int pos = s->pos;
    int k, i;
    union { unsigned int u; float f; } c; /* a tap: its bits from hb_coef_bits (halfband.h) */

    for (k = 0; k < n48; k++) {
        float acc = 0.0f;
        /* newest sample x[k] at index `pos`; x[k-j] at (pos - j) & mask */
        s->buf[pos] = in48[k];
        for (i = 0; i < HB_PAIRS; i++) {
            c.u = hb_coef_bits[i];
            acc += c.f * (s->buf[(pos - i) & mask]
                                 + s->buf[(pos - (HB_HIST - 1) + i) & mask]);
        }
        out96[2 * k] = acc;
        out96[2 * k + 1] = s->buf[(pos - HB_ODD_DELAY) & mask];
        pos = (pos + 1) & mask;
    }
    s->pos = pos;
}
