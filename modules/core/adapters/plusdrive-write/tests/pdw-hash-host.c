/* Host test of src/cf/dt2/pdw_hash.h: the vectors are packages/dksample's hashlittle (content hash, seed 0x43fa243a) of the bytes
 * (i * 7 + (i >> 8)) & 255, generated once from dksample. Every vector is also fed in odd pieces, and the 32 KiB pieces of the adapter's
 * readback. */
#include <stdio.h>
#include <stdlib.h>
#include "cf/dt2/pdw_hash.h"

static const struct {
    uint32_t n, hash;
} vec[] = {
    {1, 0x525aa0e6u},     {2, 0x08fa0133u},     {3, 0x2607e71bu},     {4, 0x3ffcef06u},
    {5, 0xca9e001au},     {8, 0x6893d19fu},     {9, 0xd2f860bfu},     {11, 0x9a30e701u},
    {12, 0xb75dd23bu},    {13, 0xc2bc6841u},    {16, 0xb44068d3u},    {17, 0x046cbbe6u},
    {24, 0x969dc6ecu},    {25, 0x3fa78d9du},    {28, 0x0ad5f2d0u},    {36, 0x6588eb9bu},
    {37, 0x7cfe0046u},    {100, 0x3b4ac3f9u},   {1199, 0xd502f734u},  {1200, 0x1ab0427du},
    {1201, 0xb7d57f69u},  {1212, 0x1002d59eu},  {4096, 0x98e1bb9cu},  {32767, 0xccb35558u},
    {32768, 0xc5cef572u}, {32769, 0x26d83376u}, {32784, 0x71502b49u}, {70016, 0xe70f17ecu},
};

static uint8_t data[70016];

static uint32_t fed(uint32_t n, uint32_t piece)
{
    struct pdw_hash h;
    uint32_t at = 0;
    pdw_hash_init(&h);
    while (at < n) {
        uint32_t k = n - at < piece ? n - at : piece;
        pdw_hash_update(&h, data + at, k);
        at += k;
    }
    return pdw_hash_finish(&h);
}

/* The textbook byte-wise lookup3 of the same variant (the seed in all three words, the last 1 to 12 bytes as the tail), apart from pdw_hash.h. */
static uint32_t rotl(uint32_t x, uint32_t k) { return (x << k) | (x >> (32u - k)); }
static uint32_t wb(const uint8_t *p, uint32_t n)
{
    uint32_t w = 0, i;
    for (i = 0; i < n; i++) w |= (uint32_t)p[i] << (24u - 8u * i);
    return w;
}
static uint32_t reference(const uint8_t *d, uint32_t n)
{
    uint32_t a = PDW_HASH_SEED, b = a, c = a, m;
    if (n == 0) return c;
    while (n > 12) {
        a += wb(d, 4); b += wb(d + 4, 4); c += wb(d + 8, 4);
        a -= c; a ^= rotl(c, 4);  c += b;
        b -= a; b ^= rotl(a, 6);  a += c;
        c -= b; c ^= rotl(b, 8);  b += a;
        a -= c; a ^= rotl(c, 16); c += b;
        b -= a; b ^= rotl(a, 19); a += c;
        c -= b; c ^= rotl(b, 4);  b += a;
        d += 12;
        n -= 12;
    }
    m = n;
    a += wb(d, m < 4 ? m : 4);
    if (m > 4) b += wb(d + 4, m < 8 ? m - 4 : 4);
    if (m > 8) c += wb(d + 8, m - 8);
    c ^= b; c -= rotl(b, 14);
    a ^= c; a -= rotl(c, 11);
    b ^= a; b -= rotl(a, 25);
    c ^= b; c -= rotl(b, 16);
    a ^= c; a -= rotl(c, 4);
    b ^= a; b -= rotl(a, 14);
    c ^= b; c -= rotl(b, 24);
    return c;
}

/* Random lengths fed in random pieces from random alignments against the reference. */
static int random_cases(void)
{
    static uint32_t backing[20000 / 4 + 8];
    uint8_t *base = (uint8_t *)backing;
    uint32_t x = 0x2545f491u, round, i;
#define RND(m) (x = x * 1664525u + 1013904223u, (x >> 8) % (m))
    for (round = 0; round < 4000; round++) {
        uint32_t n = round % 10 == 0 ? RND(19000u) : RND(300u), skew = RND(4u), at = 0;
        struct pdw_hash h;
        uint8_t *d = base + skew;
        for (i = 0; i < n; i++) d[i] = (uint8_t)RND(256u);
        pdw_hash_init(&h);
        while (at < n) {
            uint32_t k;
            switch (RND(4u)) {
            case 0: k = 1 + RND(13u); break;
            case 1: k = 1 + RND(40u); break;
            case 2: k = 512u * (1 + RND(4u)); break;
            default: k = 1 + RND(5000u); break;
            }
            if (k > n - at) k = n - at;
            pdw_hash_update(&h, d + at, k);
            at += k;
        }
        if (pdw_hash_finish(&h) != reference(d, n)) {
            fprintf(stderr, "FAIL random round %u len %u skew %u\n", round, n, skew);
            return 1;
        }
    }
    return 0;
}

int main(void)
{
    static const uint32_t pieces[] = {1, 3, 7, 12, 13, 100, 4096, 32768, 70016};
    uint32_t i, j, bad = 0;
    for (i = 0; i < sizeof data; i++)
        data[i] = (uint8_t)((i * 7u + (i >> 8)) & 255u);
    for (i = 0; i < sizeof vec / sizeof vec[0]; i++)
        for (j = 0; j < sizeof pieces / sizeof pieces[0]; j++) {
            uint32_t got = fed(vec[i].n, pieces[j]);
            if (got != vec[i].hash) {
                fprintf(stderr, "FAIL n=%u piece=%u got %08x want %08x\n", vec[i].n, pieces[j], got,
                        vec[i].hash);
                bad = 1;
            }
        }
    return bad | random_cases();
}
