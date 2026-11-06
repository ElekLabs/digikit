/* A bounded stdin/stdout adapter for the same production decoder, not a second codec. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "asset.h"

static struct {
    uint8_t before[16];
    Dkbs state;
    uint8_t after[16];
} guarded;
static uint8_t input[65537], output[16 + 300 * 1024];

static int guards(void)
{
    for (unsigned i = 0; i < 16; i++)
        if (guarded.before[i] != 0xa5 || guarded.after[i] != 0x5a)
            return 0;
    return 1;
}

static void be32(uint8_t *p, uint32_t value)
{
    for (unsigned i = 0; i < 4; i++)
        p[i] = (uint8_t)(value >> (24 - 8 * i));
}

int main(void)
{
    memset(guarded.before, 0xa5, 16);
    memset(guarded.after, 0x5a, 16);
    uint32_t size = (uint32_t)fread(input, 1, sizeof(input), stdin);
    if (!dkbs_open(&guarded.state, input, size))
        return guards() && dkbs_next(&guarded.state) == -1 ? 1 : 2;
    if (!guards() || guarded.state.tick || guarded.state.decoded_records)
        return 2;
    uint32_t ticks = 0;
    int next;
    while ((next = dkbs_next(&guarded.state)) == 1) {
        if (!guards() || ticks >= 300)
            return 2;
        memcpy(output + 16 + ticks * 1024, guarded.state.pixels, 1024);
        ticks++;
    }
    if (!guards() || next != 0 || dkbs_next(&guarded.state) != 0)
        return 2;
    be32(output, guarded.state.records);
    be32(output + 4, ticks);
    be32(output + 8, guarded.state.decoded_records);
    be32(output + 12, sizeof(Dkbs));
    return fwrite(output, 1, 16 + ticks * 1024, stdout) == 16 + ticks * 1024 ? 0 : 2;
}
