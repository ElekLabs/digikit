/* Host test of src/cf/peer.c: peer_sha256 against known vectors and (given a path and the stock hash) the real section 7, and peer_load against stubs of the
 * two firmware calls. The guest addresses map into an array (PEER_HOST); the section is 1000 bytes and the expected hash is set at run time. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dk_fw_peer.h"

/* stubs of the firmware's NOR read and depacker */
static struct {
    int calls;
    uint32_t src, len;
    uint8_t *dst;
} nor;
static const uint8_t *nor_data;
static struct {
    int calls;
    const uint8_t *src;
} dep;
static const uint8_t *dep_out;
static uint32_t dep_ret;
static uint32_t dep_len;

void dk_fw_peer_nor_read(uint32_t src, uint32_t len, uint8_t *dst)
{
    nor.calls++;
    nor.src = src;
    nor.len = len;
    nor.dst = dst;
    if (nor_data)
        memcpy(dst, nor_data, len);
}

uint32_t dk_fw_peer_depack(const uint8_t *src, uint8_t *dst)
{
    dep.calls++;
    dep.src = src;
    memcpy(dst, dep_out, dep_len);
    return dep_ret;
}

#define PEER_HOST
#define PEER_LEN 1000u
#define PEER_CONST
static uint8_t arena[0x60000 + 0x1000];
#define ARENA_BASE 0x4f84f2c1u
#define PEER_AT(a) (arena + ((a) - ARENA_BASE))
#define PEER_ADDR(p) ((uint32_t)((const uint8_t *)(p) - arena) + ARENA_BASE)
#include "peer.c"

static int bad;
#define CHECK(c)                                                                                   \
    do {                                                                                           \
        if (!(c)) {                                                                                \
            fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #c);                                   \
            bad = 1;                                                                               \
        }                                                                                          \
    } while (0)

static void hex(const uint8_t *d, char out[65])
{
    int i;
    for (i = 0; i < 32; i++)
        sprintf(out + 2 * i, "%02x", d[i]);
}

static void vector(const char *msg, const char *want)
{
    uint8_t d[32];
    char h[65];
    peer_sha256((const uint8_t *)msg, (uint32_t)strlen(msg), d);
    hex(d, h);
    if (strcmp(h, want)) {
        fprintf(stderr, "FAIL sha256(\"%s\") = %s\n", msg, h);
        bad = 1;
    }
}

static uint8_t out_good[PEER_LEN], out_bad[PEER_LEN], stream[208];
static const uint32_t PASS_SRC = 0x1234, NOR_SRC = 0x80000 + 0x4321;
static uint32_t extended_length;
static int extend_calls;

static u32 extend(u8 *image, u32 length, u32 capacity)
{
    ++extend_calls;
    CHECK(image == PEER_AT(PEER_DEST));
    CHECK(length == PEER_LEN && capacity == PEER_STAGE_END - PEER_DEST);
    CHECK(memcmp(image, out_good, PEER_LEN) == 0);
    if (extended_length == length + 4)
        memset(image + length, 0xf0, 4);
    return extended_length;
}

u32 (*const peer_image_extender)(u8 *, u32, u32) = extend;

static int load(uint32_t len)
{
    memset(arena, 0xee, 8);
    memset(&nor, 0, sizeof nor);
    memset(&dep, 0, sizeof dep);
    extend_calls = 0;
    return peer_load(NOR_SRC, len, PEER_AT(PEER_DEST));
}

static int length_word_untouched(void)
{
    int i;
    for (i = 0; i < 4; i++)
        if (PEER_AT(PEER_LEN_WORD)[i] != 0xee)
            return 0;
    return 1;
}

static void load_cases(void)
{
    uint32_t i, sum = 0, len = sizeof stream, stage = PEER_STAGE_END - len;
    for (i = 0; i < PEER_LEN; i++)
        out_good[i] = (uint8_t)(i * 31u + (i >> 3));
    memcpy(out_bad, out_good, sizeof out_bad);
    out_bad[500] ^= 1; /* one bit */
    for (i = 8; i < len; i++) {
        stream[i] = (uint8_t)(i * 7u + 3u);
        sum += stream[i];
    }
    stream[0] = 0;
    stream[1] = 0;
    stream[2] = 0;
    stream[3] = (uint8_t)(len - 8);
    stream[4] = (uint8_t)(sum >> 24);
    stream[5] = (uint8_t)(sum >> 16);
    stream[6] = (uint8_t)(sum >> 8);
    stream[7] = (uint8_t)sum;
    peer_sha256(out_good, PEER_LEN, PEER_SHA256); /* the "stock" hash of this test */
    nor_data = stream;
    dep_out = out_good;
    dep_ret = PEER_LEN;
    dep_len = PEER_LEN;
    extended_length = PEER_LEN;

    /* the section 6 read goes to the NOR read unchanged */
    memset(&nor, 0, sizeof nor);
    nor_data = NULL;
    CHECK(peer_load(PASS_SRC, 50, PEER_AT(0x4f8af2c9u)) == 0);
    CHECK(nor.calls == 1 && nor.src == PASS_SRC && nor.len == 50 &&
          nor.dst == PEER_AT(0x4f8af2c9u));
    nor_data = stream;

    /* section 7: read to the end of the buffer, unpacked to the payload, the length stored big endian */
    CHECK(load(len) == 0);
    CHECK(nor.calls == 1 && nor.src == NOR_SRC && nor.len == len && nor.dst == PEER_AT(stage));
    CHECK(dep.calls == 1 && dep.src == PEER_AT(stage));
    CHECK(PEER_AT(PEER_LEN_WORD)[0] == 0 && PEER_AT(PEER_LEN_WORD)[1] == 0 &&
          PEER_AT(PEER_LEN_WORD)[2] == (PEER_LEN >> 8) &&
          PEER_AT(PEER_LEN_WORD)[3] == (PEER_LEN & 255));
    CHECK(memcmp(PEER_AT(PEER_DEST), out_good, PEER_LEN) == 0);

    extended_length = PEER_LEN + 4;
    CHECK(load(len) == 0 && extend_calls == 1);
    CHECK(be32(PEER_AT(PEER_LEN_WORD)) == extended_length);
    CHECK(be32(PEER_AT(PEER_DEST) + PEER_LEN) == 0xf0f0f0f0u);
    extended_length = PEER_STAGE_END - PEER_DEST + 1;
    CHECK(load(len) == 1 && extend_calls == 1 && length_word_untouched());
    extended_length = PEER_LEN - 1;
    CHECK(load(len) == 1 && extend_calls == 1 && length_word_untouched());
    extended_length = PEER_LEN;

    /* every check refuses and leaves the length word alone */
    stream[3]++; /* header length */
    CHECK(load(len) == 1 && dep.calls == 0 && length_word_untouched());
    stream[3]--;
    stream[7] ^= 1; /* byte sum */
    CHECK(load(len) == 1 && dep.calls == 0 && length_word_untouched());
    stream[7] ^= 1;
    CHECK(load(7) == 1 && nor.calls == 0 && length_word_untouched());
    CHECK(load(PEER_STAGE_END - PEER_DEST + 1) == 1 && nor.calls == 0 && length_word_untouched());
    dep_ret = PEER_LEN - 1; /* output length */
    CHECK(load(len) == 1 && dep.calls == 1 && extend_calls == 0 && length_word_untouched());
    dep_ret = PEER_LEN;
    dep_out = out_bad; /* one flipped bit in the output: the hash */
    CHECK(load(len) == 1 && dep.calls == 1 && extend_calls == 0 && length_word_untouched());
    dep_out = out_good;
    CHECK(load(len) == 0); /* and the good one still passes */
}

int main(int argc, char **argv)
{
    vector("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    vector("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    vector("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
           "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    load_cases();
    if (argc == 3) { /* the real section 7 and its hash */
        FILE *f = fopen(argv[1], "rb");
        uint8_t *d, o[32];
        char h[65];
        long n;
        if (!f)
            return 2;
        fseek(f, 0, SEEK_END);
        n = ftell(f);
        fseek(f, 0, SEEK_SET);
        d = malloc((size_t)n);
        if (!d || fread(d, 1, (size_t)n, f) != (size_t)n)
            return 2;
        fclose(f);
        peer_sha256(d, (uint32_t)n, o);
        hex(o, h);
        CHECK(strcmp(h, argv[2]) == 0);
        free(d);
    }
    return bad;
}
