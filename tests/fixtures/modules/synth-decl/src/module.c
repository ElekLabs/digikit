/* The code of the synthetic capacity modules (packages/patchkit/src/dkmsynth.rs): init reads the declaration table `synth_decl` of its own data
   and makes the builder calls for it. The packer replaces the data of `synth_decl`; the words below only give the object its shape.

   Table (32-bit words, in the byte order of the CPU):
     word 0                machine count n
     n machine records:    index (DK_MACHINE_ID_<ID>: the row of its key in the keys table), parameter count p, then p parameter records
     parameter record:     min, max, default (knob units)
   The machine's name, its page and its parameters are named after their index (M3, page M3, P0, P1 ...); every machine has one page. */
#include <dk/core/module.h>

uint32_t synth_decl[4] __attribute__((aligned(4))) = {1, 0, 0, 0};

/* "<prefix><n>" into out (at most 5 characters of digits), returns out */
static char *label(char *out, char prefix, uint32_t n)
{
    char digits[6];
    int d = 0, o = 0;
    out[o++] = prefix;
    do {
        digits[d++] = (char)('0' + n % 10);
        n /= 10;
    } while (n && d < 5);
    while (d)
        out[o++] = digits[--d];
    out[o] = 0;
    return out;
}

static int32_t init(dk_module_ctx *ctx)
{
    const uint32_t *t = synth_decl;
    uint32_t machines = *t++;
    for (uint32_t i = 0; i < machines; i++) {
        char name[8], pname[8];
        uint32_t index = *t++, params = *t++;
        dk_machine *m = dk_module_ctx_add_machine(ctx, index, label(name, 'M', i), name);
        dk_page *pg = dk_machine_add_page(m, name, name);
        for (uint32_t j = 0; j < params; j++, t += 3) {
            dk_param *p = dk_page_add_param(pg, label(pname, 'P', j), pname, t[2]);
            dk_param_range(p, t[0], t[1]);
        }
    }
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
