/* core/peer-image: MAIN unpacks the second CPU's program before it streams it (docs/findings/151-syntakt-section7-pack.md). ColdFire, C.
 *
 * The peer thread of the stock MAIN reads two NOR payloads through one function, `read(src, len, dst)`: section 6 into the transmit buffer behind
 * the section 6 header, then section 7 (the second CPU's program) to `PEER_DEST`. The stream to the second CPU is the bytes of that buffer, so the
 * second CPU normally gets the stock bytes of section 7. patchkit stores section 7 packed (about 120 KB smaller: the stock image is over the limit
 * of the OS upgrade stage) and a hook gives the thread this wrapper instead of the NOR read (hook.s).
 *
 * peer_load(src, len, dst), called from peer_read (hook.s) with the thread's own arguments:
 *   dst is not PEER_DEST (the section 6 read): the stock NOR read, unchanged.
 *   dst is PEER_DEST: read the packed stream to the end of the buffer (PEER_STAGE_END - len), check its header (length and byte sum), unpack it
 *     to PEER_DEST, verify its length and SHA-256, allow the optional extender to update the staged image, and store the output length where the thread stored the
 *     packed length. Any failed check returns 1: peer_read then takes the stock path of a missing section 7 (status 1, the boot failure screen).
 *     The DMA that streams the buffer is armed later, so a failed check streams nothing.
 * The addresses and the expected length and hash are the `[[repack]]` of module.toml, through the generated build/gen/repack.h: patchkit checks the stored
 * stream's in-place margin against them and the decoded section against the hash.
 * tests/peer-host.test.ts compiles this file for the host (PEER_HOST) and runs peer_sha256 and peer_load against stubs of the two firmware calls. */

#include <stdint.h>
#include "dk_fw_peer.h"
#include "repack.h" /* build/gen/repack.h: the [[repack]] of module.toml (patchkit gen) */

typedef uint32_t u32;
typedef uint8_t u8;

/* The layout and the stock section come from the manifest's [[repack]] of role cf2 (one source: patchkit checks the stored stream against the same values).
 * The thread stores the big-endian length of the payload 4 bytes below it, behind the 0xAA byte. */
#define PEER_DEST REPACK_CF2_DEST
#define PEER_STAGE_END REPACK_CF2_STAGE_END
#define PEER_LEN_WORD (REPACK_CF2_DEST - 4u)
#ifndef PEER_LEN /* the host test uses a short section */
#define PEER_LEN REPACK_CF2_LEN
#endif

/* The optional extender sees the verified decoded image before the peer starts. */
extern u32 (*const peer_image_extender)(u8 *image, u32 length, u32 capacity);

/* Guest addresses to pointers and back. The host test defines PEER_HOST and maps them into an array. */
#ifndef PEER_HOST
#define PEER_CONST const
#define PEER_AT(a) ((u8 *)(a))
#define PEER_ADDR(p) ((u32)(p))
#endif

/* SHA-256 of the stock section 7 */
static PEER_CONST u8 PEER_SHA256[32] = REPACK_CF2_SHA256;

static const u32 SHA_K[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u,
    0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu,
    0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu,
    0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau, 0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu,
    0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
    0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u, 0x19a4c116u,
    0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u,
    0xc67178f2u,
};

static u32 be32(const u8 *p)
{
    return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | p[3];
}

static u32 ror(u32 x, unsigned n)
{
    return (x >> n) | (x << (32 - n));
}

/* One 64-byte block; the message schedule is a ring of 16 words. */
static void sha_block(u32 h[8], const u8 *p)
{
    u32 w[16];
    u32 v[8];
    unsigned i, j;
    for (j = 0; j < 8; j++)
        v[j] = h[j];
    for (i = 0; i < 64; i++) {
        u32 t1, t2;
        if (i < 16) {
            w[i] = be32(p + 4 * i);
        } else {
            u32 a = w[(i + 1) & 15], b = w[(i + 14) & 15];
            w[i & 15] += (ror(a, 7) ^ ror(a, 18) ^ (a >> 3)) + w[(i + 9) & 15] +
                         (ror(b, 17) ^ ror(b, 19) ^ (b >> 10));
        }
        t1 = v[7] + (ror(v[4], 6) ^ ror(v[4], 11) ^ ror(v[4], 25)) +
             ((v[4] & v[5]) ^ (~v[4] & v[6])) + SHA_K[i] + w[i & 15];
        t2 = (ror(v[0], 2) ^ ror(v[0], 13) ^ ror(v[0], 22)) +
             ((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
        for (j = 7; j > 0; j--)
            v[j] = v[j - 1];
        v[4] += t1;
        v[0] = t1 + t2;
    }
    for (j = 0; j < 8; j++)
        h[j] += v[j];
}

/* SHA-256 of `len` bytes at `data`. Not static: the host test calls it. */
void peer_sha256(const u8 *data, u32 len, u8 out[32])
{
    u32 h[8];
    u8 tail[128];
    u32 full = len & ~63u, rest = len - full, n, i;
    /* single stores: no libc, so no initializer that the compiler turns into a memcpy */
    h[0] = 0x6a09e667u;
    h[1] = 0xbb67ae85u;
    h[2] = 0x3c6ef372u;
    h[3] = 0xa54ff53au;
    h[4] = 0x510e527fu;
    h[5] = 0x9b05688cu;
    h[6] = 0x1f83d9abu;
    h[7] = 0x5be0cd19u;
    for (i = 0; i < full; i += 64)
        sha_block(h, data + i);
    for (i = 0; i < rest; i++)
        tail[i] = data[full + i];
    tail[rest] = 0x80;
    n = rest < 56 ? 64 : 128;
    for (i = rest + 1; i < n - 8; i++)
        tail[i] = 0;
    /* the bit length: len is below 2^29 here, so the high word is 0 */
    tail[n - 8] = 0;
    tail[n - 7] = 0;
    tail[n - 6] = 0;
    tail[n - 5] = (u8)(len >> 29);
    tail[n - 4] = (u8)(len >> 21);
    tail[n - 3] = (u8)(len >> 13);
    tail[n - 2] = (u8)(len >> 5);
    tail[n - 1] = (u8)(len << 3);
    for (i = 0; i < n; i += 64)
        sha_block(h, tail + i);
    for (i = 0; i < 32; i++)
        out[i] = (u8)(h[i >> 2] >> (24 - 8 * (i & 3)));
}

/* 0: the section 6 read was done, or section 7 is ready at PEER_DEST. 1: a check failed. */
int peer_load(u32 src, u32 len, u8 *dst)
{
    u8 *p;
    u8 sha[32];
    u32 i, sum = 0, n;
    if (PEER_ADDR(dst) != PEER_DEST) {
        dk_fw_peer_nor_read(src, len, dst);
        return 0;
    }
    if (len < 8 || len > PEER_STAGE_END - PEER_DEST)
        return 1;
    p = PEER_AT(PEER_STAGE_END - len);
    dk_fw_peer_nor_read(src, len, p);
    if (be32(p) != len - 8)
        return 1;
    for (i = 8; i < len; i++)
        sum += p[i];
    if (sum != be32(p + 4))
        return 1;
    n = dk_fw_peer_depack(p, dst);
    if (n != PEER_LEN)
        return 1;
    peer_sha256(dst, PEER_LEN, sha);
    for (i = 0; i < 32; i++)
        if (sha[i] != PEER_SHA256[i])
            return 1;
    if (peer_image_extender) {
        n = peer_image_extender(dst, PEER_LEN, PEER_STAGE_END - PEER_DEST);
        if (n < PEER_LEN || n > PEER_STAGE_END - PEER_DEST)
            return 1;
    }
    *PEER_AT(PEER_LEN_WORD + 0) = (u8)(n >> 24);
    *PEER_AT(PEER_LEN_WORD + 1) = (u8)(n >> 16);
    *PEER_AT(PEER_LEN_WORD + 2) = (u8)(n >> 8);
    *PEER_AT(PEER_LEN_WORD + 3) = (u8)n;
    return 0;
}
