/* core/peer-image: MAIN unpacks the second CPU's program before it streams it (docs/findings/150-syntakt-section7-pack.md). ColdFire, C.
 *
 * The peer thread of the stock MAIN reads two NOR payloads through one function, `read(src, len, dst)`: section 6 into the transmit buffer behind
 * the section 6 header, then section 7 (the second CPU's program) to `PEER_DEST`. The stream to the second CPU is the bytes of that buffer, so the
 * second CPU must get the stock bytes of section 7. patchkit stores section 7 packed (about 120 KB smaller: the stock image is over the limit
 * of the OS upgrade stage) and a hook gives the thread this wrapper instead of the NOR read (hook.s).
 *
 * peer_load(src, len, dst), called from peer_read (hook.s) with the thread's own arguments:
 *   dst is not PEER_DEST (the section 6 read): the stock NOR read, unchanged.
 *   dst is PEER_DEST: read the packed stream to the end of the buffer (PEER_STAGE_END - len), check its header (length and byte sum), unpack it
 *     to PEER_DEST, check the output length and its SHA-256 against the stock section, and store the output length where the thread stored the
 *     packed length. Any failed check returns 1: peer_read then takes the stock path of a missing section 7 (status 1, "DSP BOOT FAILURE").
 *     The DMA that streams the buffer is armed later, so a failed check streams nothing.
 * The addresses and the expected length and hash are the `[[repack]]` of module.toml: patchkit checks the stored stream's in-place margin against
 * them and the decoded section against the hash; tests/layout.test.ts checks that this file names the same values. */

#include <stdint.h>
#include "dk_fw_peer.h"

typedef uint32_t u32;
typedef uint8_t u8;

/* Where the stock thread reads section 7 (uncached alias), the end of its buffer (0x4f84f2c1 + 0x60000), the big-endian length word the thread
 * stores 4 bytes below the payload, and the length of the stock section 7. */
#define PEER_DEST 0x4f84f2c6u
#define PEER_STAGE_END 0x4f8af2c1u
#define PEER_LEN_WORD 0x4f84f2c2u
#define PEER_LEN 383760u

/* SHA-256 of the stock section 7 */
static const u8 PEER_SHA256[32] = {
    0xda, 0xf6, 0x45, 0x1c, 0xf9, 0x58, 0x7c, 0x0b, 0x62, 0x8e, 0x90, 0x1b, 0x7b, 0xb6, 0xb2, 0x5f,
    0x4e, 0x26, 0x33, 0xd4, 0x48, 0xc5, 0x34, 0xc0, 0xc1, 0x81, 0xdd, 0x35, 0xec, 0x78, 0x3b, 0xc2,
};

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

/* SHA-256 of `len` bytes at `data`. Not static: the host check links it. */
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

/* 0: the section 6 read was done, or the stock section 7 is unpacked at PEER_DEST. 1: a check failed. */
int peer_load(u32 src, u32 len, u8 *dst)
{
    u8 *p;
    u8 sha[32];
    u32 i, sum = 0, n;
    if ((u32)dst != PEER_DEST) {
        dk_fw_peer_nor_read(src, len, dst);
        return 0;
    }
    if (len < 8 || len > PEER_STAGE_END - PEER_DEST)
        return 1;
    p = (u8 *)(PEER_STAGE_END - len);
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
    *(u8 *)(PEER_LEN_WORD + 0) = (u8)(n >> 24);
    *(u8 *)(PEER_LEN_WORD + 1) = (u8)(n >> 16);
    *(u8 *)(PEER_LEN_WORD + 2) = (u8)(n >> 8);
    *(u8 *)(PEER_LEN_WORD + 3) = (u8)n;
    return 0;
}
