/* The LFO shapes of digikit/lfo_shapes_1. Each entry of SHAPES is one `lfo_shape` record: a 4-character name and 32 samples of signed 16-bit (Q15), one period. `init` adds the records
   with dk_record_add. The `lfo` service numbers each shape (a value of the LFO WAVE parameter, 7 and up) and the LFO tick reads the samples with linear interpolation,
   the last sample leading back to the first. Saved sounds keep a shape by its number; the number follows the key, which derives from the module id and the record id in
   module.toml: never rename either.
   KEYS[i] is the key of SHAPES[i]: list one `[[record]] kind = "lfo_shape"` in module.toml for each shape and its DK_KEY_<ID> here, in the same order.
   `mise run module:check` refuses a module with more than 8 shapes, a name twice, names that share their first 3 characters (the Digitone II shows 3), a flat or all-zero shape.
   A shape with no negative sample is unipolar (like the stock EXPO and RAMP). Write the samples as numbers: the README lists the formula of each. */
#include <dk/core/module.h>
#include <dk/gen/records.h>
#include "module_ids.h"

static const struct {
    const char name[4]; /* the name on the MOD page: 4 characters, space to tilde, no terminator */
    const int16_t s[32];
} SHAPES[] = {
    {
        {'S', 'T', 'P', '4'},
        {
            -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -10000, -10000, -10000,
            -10000, -10000, -10000, -10000, -10000, 10000,  10000,  10000,  10000,  10000,  10000,
            10000,  10000,  30000,  30000,  30000,  30000,  30000,  30000,  30000,  30000,
        },
    },
    {
        {'P', 'U', 'L', 'S'},
        {
            30000,  30000,  30000,  30000,  30000,  30000,  30000,  30000,  -30000, -30000, -30000,
            -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000,
            -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000, -30000,
        },
    },
    {
        {'H', 'S', 'I', 'N'},
        {
            0,     5853,  11481, 16667, 21213, 24944, 27716, 29424, 30000, 29424, 27716,
            24944, 21213, 16667, 11481, 5853,  0,     0,     0,     0,     0,     0,
            0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
        },
    },
    {
        {'A', 'B', 'S', 'N'},
        {
            0,     5853,  11481, 16667, 21213, 24944, 27716, 29424, 30000, 29424, 27716,
            24944, 21213, 16667, 11481, 5853,  0,     5853,  11481, 16667, 21213, 24944,
            27716, 29424, 30000, 29424, 27716, 24944, 21213, 16667, 11481, 5853,
        },
    },
    {
        {'S', 'O', 'F', 'T'},
        {
            0,      9364,   18369,  26667,  30000,  30000,  30000,  30000,  30000,  30000,  30000,
            30000,  30000,  26667,  18369,  9364,   0,      -9364,  -18369, -26667, -30000, -30000,
            -30000, -30000, -30000, -30000, -30000, -30000, -30000, -26667, -18369, -9364,
        },
    },
    {
        {'D', 'U', 'C', 'K'},
        {
            -30000, -21321, -13897, -7547, -2116, 2530,  6504,  9903,  12810, 15296, 17423,
            19243,  20799,  22130,  23268, 24242, 25075, 25787, 26397, 26918, 27364, 27745,
            28071,  28350,  28589,  28793, 28968, 29117, 29245, 29354, 29447, 29527,
        },
    },
    {
        {'W', 'O', 'B', 'L'},
        {
            0,      8943,   16119,  20312,  21213,  19489,  16517,  13890,  12857,  13890,  16517,
            19489,  21213,  20312,  16119,  8943,   0,      -8943,  -16119, -20312, -21213, -19489,
            -16517, -13890, -12857, -13890, -16517, -19489, -21213, -20312, -16119, -8943,
        },
    },
};

static const uint32_t KEYS[] = {DK_KEY_STP4, DK_KEY_PULS, DK_KEY_HSIN, DK_KEY_ABSN,
                                DK_KEY_SOFT, DK_KEY_DUCK, DK_KEY_WOBL};

#define N_SHAPES ((uint32_t)(sizeof SHAPES / sizeof SHAPES[0]))
/* the compile fails when there are no shapes, more than 8, or a different number of keys */
typedef char shapes_are_1_to_8[(N_SHAPES >= 1 && N_SHAPES <= 8) ? 1 : -1];
typedef char one_key_per_shape[(sizeof KEYS / sizeof KEYS[0] == N_SHAPES) ? 1 : -1];

static int32_t init(dk_module_ctx *ctx)
{
    uint32_t n;
    for (n = 0; n < N_SHAPES; n++) {
        uint8_t body[DK_RECORD_LEN_LFO_SHAPE - 8];
        uint32_t i;
        int32_t rc;
        /* the key, big-endian */
        body[0] = (uint8_t)(KEYS[n] >> 24);
        body[1] = (uint8_t)(KEYS[n] >> 16);
        body[2] = (uint8_t)(KEYS[n] >> 8);
        body[3] = (uint8_t)KEYS[n];
        for (i = 0; i < 4; i++)
            body[4 + i] = (uint8_t)SHAPES[n].name[i];
        for (i = 0; i < 32; i++) {
            body[8 + 2 * i] = (uint8_t)((uint16_t)SHAPES[n].s[i] >> 8);
            body[9 + 2 * i] = (uint8_t)SHAPES[n].s[i];
        }
        rc = dk_record_add(ctx, DK_RECORD_KIND_LFO_SHAPE, body, sizeof body);
        if (rc != DK_OK)
            return rc; /* stop at the first error */
    }
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
