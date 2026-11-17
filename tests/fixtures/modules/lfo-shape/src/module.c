/* test/lfo-shape: one lfo_shape record (R7): its key and 32 samples, a ramp from -1000 in steps of 100. The `lfo` service owns the kind. */
#include <dk/core/module.h>
#include <dk/gen/records.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    uint8_t body[DK_RECORD_LEN_LFO_SHAPE - 8];
    uint32_t i;
    body[0] = (uint8_t)(DK_KEY_SHAPE >> 24); /* the shape id is the key, big-endian */
    body[1] = (uint8_t)(DK_KEY_SHAPE >> 16);
    body[2] = (uint8_t)(DK_KEY_SHAPE >> 8);
    body[3] = (uint8_t)DK_KEY_SHAPE;
    for (i = 0; i < 32; i++) {
        int32_t v = (-1000) + 100 * (int32_t)i;
        body[4 + 2 * i] = (uint8_t)((uint32_t)v >> 8);
        body[5 + 2 * i] = (uint8_t)v;
    }
    return dk_record_add(ctx, DK_RECORD_KIND_LFO_SHAPE, body, sizeof body);
}

const dk_module_def dk_module = {.init = init};
