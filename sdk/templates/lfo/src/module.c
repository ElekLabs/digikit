/* The LFO shape of __ID__: one `lfo_shape` record that `init` adds with dk_record_add. The `lfo` service numbers the shape (a value of the LFO WAVE parameter, 7 and up)
   and the LFO tick reads its 32 samples with linear interpolation. Saved sounds keep the shape by its number; the number is kept by the key, which derives from the module id and
   the record id `shape` of module.toml: never rename either. */
#include <dk/core/module.h>
#include <dk/gen/records.h>
#include "module_ids.h"

/* One period, 32 samples of signed 16-bit (Q15): replace them with your own. A shape that has no negative sample is unipolar (like the stock EXPO and RAMP). */
static const int16_t samples[32] = {
    0,      6393,   12539,  18204,  23170,  27245,  30273,  32137,  32767,  32137,  30273,
    27245,  23170,  18204,  12539,  6393,   0,      -6393,  -12539, -18204, -23170, -27245,
    -30273, -32137, -32767, -32137, -30273, -27245, -23170, -18204, -12539, -6393,
};

static int32_t init(dk_module_ctx *ctx)
{
    uint8_t body[DK_RECORD_LEN_LFO_SHAPE - 8];
    uint32_t i;
    /* the key, big-endian: DK_KEY_<id of module.toml> */
    body[0] = (uint8_t)(DK_KEY___UPPER__ >> 24);
    body[1] = (uint8_t)(DK_KEY___UPPER__ >> 16);
    body[2] = (uint8_t)(DK_KEY___UPPER__ >> 8);
    body[3] = (uint8_t)DK_KEY___UPPER__;
    /* the name on the MOD page: 4 characters, space to tilde (the Digitone II shows the first 3) */
    body[4] = 'S';
    body[5] = 'H';
    body[6] = 'P';
    body[7] = ' ';
    for (i = 0; i < 32; i++) {
        body[8 + 2 * i] = (uint8_t)((uint16_t)samples[i] >> 8);
        body[9 + 2 * i] = (uint8_t)samples[i];
    }
    return dk_record_add(ctx, DK_RECORD_KIND_LFO_SHAPE, body, sizeof body);
}

const dk_module_def dk_module = {.init = init};
