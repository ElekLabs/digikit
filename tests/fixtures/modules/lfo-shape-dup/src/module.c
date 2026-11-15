/* test/lfo-shape-dup: one lfo_shape record (R7): the shape id 7 and 32 samples, a ramp from 500 in steps of 100. The `lfo` service owns the kind. */
#include <dk/core/module.h>
#include <dk/gen/records.h>

static int32_t init(dk_module_ctx *ctx)
{
    uint8_t body[DK_RECORD_LEN_LFO_SHAPE - 8];
    uint32_t i;
    body[0] = 0;
    body[1] = 0;
    body[2] = 0;
    body[3] = 7; /* the shape id, big-endian */
    for (i = 0; i < 32; i++) {
        int32_t v = (500) + 100 * (int32_t)i;
        body[4 + 2 * i] = (uint8_t)((uint32_t)v >> 8);
        body[5 + 2 * i] = (uint8_t)v;
    }
    return dk_record_add(ctx, DK_RECORD_KIND_LFO_SHAPE, body, sizeof body);
}

const dk_module_def dk_module = {.init = init};
