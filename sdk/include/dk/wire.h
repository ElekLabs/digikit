/* <dk/wire.h>: protobuf wire helpers for the codecs patchkit generates from a .proto schema (varint, length-delimited, skip).
 * Hand-written, header only, freestanding. The reader never reads past its end: a failure sets `err` and every later call
 * returns 0. A writer that runs out of room sets `err`; its output is then incomplete. */
#ifndef DK_WIRE_H
#define DK_WIRE_H
#include <stdint.h>

#define DK_WIRE_VARINT 0u
#define DK_WIRE_I64 1u
#define DK_WIRE_LEN 2u
#define DK_WIRE_I32 5u

/* Generated codecs use DK_WIRE_FN for their functions: static inline unless the build defines it (size measurements). */
#ifndef DK_WIRE_FN
#define DK_WIRE_FN static inline
#endif
/* The decoders branch on the field number; a jump table puts data words in the code, which the ColdFire ISA check refuses. */
#if defined(__GNUC__) && !defined(__clang__)
#define DK_WIRE_NO_TABLES __attribute__((optimize("no-jump-tables")))
#else
#define DK_WIRE_NO_TABLES
#endif

typedef struct {
    const uint8_t *p;
    const uint8_t *end;
    int32_t err;
} dk_wire_reader;

typedef struct {
    uint8_t *p;
    uint8_t *end;
    int32_t err;
} dk_wire_writer;

static inline void dk_wire_reader_init(dk_wire_reader *r, const uint8_t *p, uint32_t n)
{
    r->p = p;
    r->end = p + n;
    r->err = 0;
}

/* One varint of up to 10 bytes, truncated to 32 bits (a negative int32 arrives as 10 bytes). */
static inline uint32_t dk_wire_get_varint(dk_wire_reader *r)
{
    uint32_t v = 0;
    for (uint32_t i = 0; i < 10; i++) {
        if (r->err || r->p == r->end) {
            r->err = 1;
            return 0;
        }
        uint8_t b = *r->p++;
        if (i < 5)
            v |= (uint32_t)(b & 0x7f) << (7 * i);
        if (!(b & 0x80))
            return v;
    }
    r->err = 1;
    return 0;
}

/* The next field key: the field number, with the wire type in *wire_type. 0 at the end or on error. */
static inline uint32_t dk_wire_get_key(dk_wire_reader *r, uint32_t *wire_type)
{
    if (r->err || r->p == r->end)
        return 0;
    uint32_t key = dk_wire_get_varint(r);
    *wire_type = key & 7u;
    return key >> 3;
}

/* A length-delimited payload: its start, with the length in *n; 0 (and err) when the length passes the end. */
static inline const uint8_t *dk_wire_get_len(dk_wire_reader *r, uint32_t *n)
{
    uint32_t len = dk_wire_get_varint(r);
    if (r->err || len > (uint32_t)(r->end - r->p)) {
        r->err = 1;
        return 0;
    }
    const uint8_t *at = r->p;
    r->p += len;
    *n = len;
    return at;
}

/* Skips a field of WIRE_TYPE (groups are refused). */
static inline void dk_wire_skip(dk_wire_reader *r, uint32_t wire_type)
{
    uint32_t n;
    switch (wire_type) {
    case DK_WIRE_VARINT:
        (void)dk_wire_get_varint(r);
        break;
    case DK_WIRE_LEN:
        (void)dk_wire_get_len(r, &n);
        break;
    case DK_WIRE_I64:
    case DK_WIRE_I32:
        n = wire_type == DK_WIRE_I64 ? 8u : 4u;
        if (n > (uint32_t)(r->end - r->p))
            r->err = 1;
        else
            r->p += n;
        break;
    default:
        r->err = 1;
    }
}

static inline void dk_wire_writer_init(dk_wire_writer *w, uint8_t *p, uint32_t cap)
{
    w->p = p;
    w->end = p + cap;
    w->err = 0;
}

static inline void dk_wire_put_varint(dk_wire_writer *w, uint32_t v)
{
    do {
        if (w->p == w->end) {
            w->err = 1;
            return;
        }
        uint8_t b = v & 0x7f;
        v >>= 7;
        *w->p++ = v ? (uint8_t)(b | 0x80) : b;
    } while (v);
}

/* A negative int32 takes 10 bytes (sign-extended to 64 bits), as protobuf writes it. */
static inline void dk_wire_put_int32(dk_wire_writer *w, int32_t s)
{
    uint32_t v = (uint32_t)s;
    if (s >= 0) {
        dk_wire_put_varint(w, v);
        return;
    }
    if (w->end - w->p < 10) {
        w->err = 1;
        return;
    }
    for (uint32_t i = 0; i < 4; i++)
        *w->p++ = (uint8_t)(((v >> (7 * i)) & 0x7f) | 0x80);
    *w->p++ = (uint8_t)(((v >> 28) & 0x0f) | 0xf0);
    for (uint32_t i = 0; i < 4; i++)
        *w->p++ = 0xff;
    *w->p++ = 0x01;
}

/* Bytes of a varint / of an int32 (10 when negative). */
static inline uint32_t dk_wire_size_varint(uint32_t v)
{
    uint32_t n = 1;
    while (v >>= 7)
        n++;
    return n;
}

static inline uint32_t dk_wire_size_int32(int32_t v)
{
    return v < 0 ? 10u : dk_wire_size_varint((uint32_t)v);
}

static inline void dk_wire_put_key(dk_wire_writer *w, uint32_t field, uint32_t wire_type)
{
    dk_wire_put_varint(w, (field << 3) | wire_type);
}

/* Byte copy without a libc call (the compiler may not turn it into one). */
static inline void dk_wire_copy(void *to, const void *from, uint32_t n)
{
    uint8_t *d = (uint8_t *)to;
    const uint8_t *s = (const uint8_t *)from;
    while (n--)
        *d++ = *s++;
}

static inline void dk_wire_put_bytes(dk_wire_writer *w, const void *p, uint32_t n)
{
    if (n > (uint32_t)(w->end - w->p)) {
        w->err = 1;
        return;
    }
    dk_wire_copy(w->p, p, n);
    w->p += n;
}
#endif
