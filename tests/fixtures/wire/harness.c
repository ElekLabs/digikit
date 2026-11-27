/* Host harness of the generated query codec. stdin: one line per case, "<Message> <hex>". stdout: "REFUSE" when decode fails, else the hex
 * of the message encoded again. Built by the tests (packages/patchkit/tests/wire_roundtrip.rs, packages/wire/tests/codec.test.ts)
 * against the header `patchkit wire c --prefix pm` writes for project_manager.proto. */
#include <stdio.h>
#include <string.h>
#include "pm.h"

static int unhex(const char *s, uint8_t *out, uint32_t cap, uint32_t *n)
{
    uint32_t len = (uint32_t)strlen(s);
    if (len % 2 || len / 2 > cap)
        return -1;
    for (uint32_t i = 0; i < len / 2; i++) {
        unsigned v;
        if (sscanf(s + 2 * i, "%2x", &v) != 1)
            return -1;
        out[i] = (uint8_t)v;
    }
    *n = len / 2;
    return 0;
}

#define CASE(T)                                                                                    \
    if (!strcmp(name, #T)) {                                                                       \
        static pm_##T msg;                                                                         \
        if (pm_##T##_decode(in, n, &msg) < 0)                                                      \
            return puts("REFUSE");                                                                 \
        dk_wire_writer w;                                                                          \
        dk_wire_writer_init(&w, out, sizeof out);                                                  \
        pm_##T##_encode(&w, &msg);                                                                 \
        if (w.err || (uint32_t)(w.p - out) > pm_##T##_MAX_SIZE)                                    \
            return puts("ENCODE-ERROR");                                                           \
        for (uint8_t *p = out; p < w.p; p++)                                                       \
            printf("%02x", *p);                                                                    \
        return puts("");                                                                           \
    }

static int run(const char *name, const uint8_t *in, uint32_t n)
{
    static uint8_t out[2048];
    CASE(ListRequest)
    CASE(Project)
    CASE(ListReply)
    CASE(Coverage)
    return puts("UNKNOWN-MESSAGE");
}

int main(void)
{
    static char line[8192];
    static uint8_t in[4096];
    while (fgets(line, sizeof line, stdin)) {
        char name[32], hex[8000];
        uint32_t n = 0;
        hex[0] = 0;
        if (sscanf(line, "%31s %7999s", name, hex) < 1 || unhex(hex, in, sizeof in, &n) < 0)
            return 2;
        run(name, in, n);
    }
    return 0;
}
