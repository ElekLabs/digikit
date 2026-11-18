/* test/lfo-shape: one lfo_shape record (R7): its key, the name "RAMP" and 32 samples, a ramp from -31744 in steps of 2048 (nearly full scale, so the page graph shows it). The `lfo` service owns the kind.
   test/lfo-shape-b includes this file with its own SHAPE_NAME and SHAPE_SAMPLE (a second shape identity). */
#include <dk/core/module.h>
#include <dk/gen/records.h>
#include "module_ids.h"

#ifndef SHAPE_NAME
#define SHAPE_NAME "RAMP"
#define SHAPE_SAMPLE(i) ((-31744) + 2048 * (int32_t)(i))
#endif

static int32_t init(dk_module_ctx *ctx)
{
    uint8_t body[DK_RECORD_LEN_LFO_SHAPE - 8];
    uint32_t i;
    body[0] = (uint8_t)(DK_KEY_SHAPE >> 24); /* the shape id is the key, big-endian */
    body[1] = (uint8_t)(DK_KEY_SHAPE >> 16);
    body[2] = (uint8_t)(DK_KEY_SHAPE >> 8);
    body[3] = (uint8_t)DK_KEY_SHAPE;
    body[4] = (uint8_t)SHAPE_NAME[0]; /* the name: 4 characters, space to tilde */
    body[5] = (uint8_t)SHAPE_NAME[1];
    body[6] = (uint8_t)SHAPE_NAME[2];
    body[7] = (uint8_t)SHAPE_NAME[3];
    for (i = 0; i < 32; i++) {
        int32_t v = SHAPE_SAMPLE(i);
        body[8 + 2 * i] = (uint8_t)((uint32_t)v >> 8);
        body[9 + 2 * i] = (uint8_t)v;
    }
    return dk_record_add(ctx, DK_RECORD_KIND_LFO_SHAPE, body, sizeof body);
}

const dk_module_def dk_module = {.init = init};
