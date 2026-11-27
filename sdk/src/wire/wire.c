/* Protobuf wire helpers of the generated query codecs (declarations and rules: <dk/wire.h>). */
#include <dk/wire.h>

void dk_wire_reader_init(dk_wire_reader *r, const uint8_t *p, uint32_t n)
{
    r->p = p;
    r->end = p + n;
    r->err = 0;
}

uint32_t dk_wire_get_varint(dk_wire_reader *r)
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

uint32_t dk_wire_get_key(dk_wire_reader *r, uint32_t *wire_type)
{
    if (r->err || r->p == r->end)
        return 0;
    uint32_t key = dk_wire_get_varint(r);
    *wire_type = key & 7u;
    return key >> 3;
}

const uint8_t *dk_wire_get_len(dk_wire_reader *r, uint32_t *n)
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

void dk_wire_skip(dk_wire_reader *r, uint32_t wire_type)
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

void dk_wire_writer_init(dk_wire_writer *w, uint8_t *p, uint32_t cap)
{
    w->p = p;
    w->end = p + cap;
    w->err = 0;
}

void dk_wire_put_varint(dk_wire_writer *w, uint32_t v)
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

void dk_wire_put_int32(dk_wire_writer *w, int32_t s)
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

uint32_t dk_wire_size_varint(uint32_t v)
{
    uint32_t n = 1;
    while (v >>= 7)
        n++;
    return n;
}

uint32_t dk_wire_size_int32(int32_t v)
{
    return v < 0 ? 10u : dk_wire_size_varint((uint32_t)v);
}

void dk_wire_put_key(dk_wire_writer *w, uint32_t field, uint32_t wire_type)
{
    dk_wire_put_varint(w, (field << 3) | wire_type);
}

void dk_wire_copy(void *to, const void *from, uint32_t n)
{
    uint8_t *d = (uint8_t *)to;
    const uint8_t *s = (const uint8_t *)from;
    while (n--)
        *d++ = *s++;
}

void dk_wire_put_bytes(dk_wire_writer *w, const void *p, uint32_t n)
{
    if (n > (uint32_t)(w->end - w->p)) {
        w->err = 1;
        return;
    }
    dk_wire_copy(w->p, p, n);
    w->p += n;
}

void dk_wire_zero(void *to, uint32_t n)
{
    uint8_t *d = (uint8_t *)to;
    while (n--)
        *d++ = 0;
}
