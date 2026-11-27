/* <dk/wire.h>: protobuf wire helpers for the codecs patchkit generates from a .proto schema (varint, length-delimited, skip).
 * Declarations only; the bodies are the SDK component sdk/src/wire/wire.c, which the packer links into a module that calls one.
 * Hand-written, freestanding. The reader never reads past its end: a failure sets `err` and every later call returns 0.
 * A writer that runs out of room sets `err`; its output is then incomplete. */
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

void dk_wire_reader_init(dk_wire_reader *r, const uint8_t *p, uint32_t n);
/* One varint of up to 10 bytes, truncated to 32 bits (a negative int32 arrives as 10 bytes). */
uint32_t dk_wire_get_varint(dk_wire_reader *r);
/* The next field key: the field number, with the wire type in *wire_type. 0 at the end or on error. */
uint32_t dk_wire_get_key(dk_wire_reader *r, uint32_t *wire_type);
/* A length-delimited payload: its start, with the length in *n; 0 (and err) when the length passes the end. */
const uint8_t *dk_wire_get_len(dk_wire_reader *r, uint32_t *n);
/* Skips a field of WIRE_TYPE (groups are refused). */
void dk_wire_skip(dk_wire_reader *r, uint32_t wire_type);
void dk_wire_writer_init(dk_wire_writer *w, uint8_t *p, uint32_t cap);
void dk_wire_put_varint(dk_wire_writer *w, uint32_t v);
/* A negative int32 takes 10 bytes (sign-extended to 64 bits), as protobuf writes it. */
void dk_wire_put_int32(dk_wire_writer *w, int32_t s);
/* Bytes of a varint / of an int32 (10 when negative). */
uint32_t dk_wire_size_varint(uint32_t v);
uint32_t dk_wire_size_int32(int32_t v);
void dk_wire_put_key(dk_wire_writer *w, uint32_t field, uint32_t wire_type);
/* Byte copy without a libc call (the compiler may not turn it into one). */
void dk_wire_copy(void *to, const void *from, uint32_t n);
void dk_wire_put_bytes(dk_wire_writer *w, const void *p, uint32_t n);
/* Sets n bytes to zero without a libc call. */
void dk_wire_zero(void *to, uint32_t n);
#endif
