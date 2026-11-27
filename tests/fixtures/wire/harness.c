/* Host harness of the generated query code. Built by packages/patchkit/tests/wire_roundtrip.rs
 * from the header and source the generated codec writes for project_manager.proto, and the SDK component sdk/src/wire/wire.c.
 * stdin, one line per case:
 *   "<Message> <hex>"  stdout: "REFUSE" when decode fails, else the hex of the message encoded again.
 *   "@calls"           stdout: "CALLS-OK", or "CALLS-FAIL <what>": the generated query table and call function (numbering from 1,
 *                      reply bounded by *out_len, a truncated request is DK_E_FORMAT). */
#include <stdio.h>
#include <string.h>
#include "pm.h"
#include "pm.c"

/* The handler of List, as the call checks use it: `cursor` items (at most 16), item i with slot i and a 16 byte name. */
int32_t pm_list(dk_module_ctx *ctx, const pm_ListRequest *request, pm_ListReply *reply)
{
    (void)ctx;
    for (uint32_t i = 0; i < request->cursor && i < 16; i++) {
        reply->items[i].slot = i;
        for (uint32_t k = 0; k < 16; k++)
            reply->items[i].name[k] = 'x';
        reply->items[i].name_len = 16;
    }
    reply->items_count = request->cursor < 16 ? request->cursor : 16;
    return 0;
}

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

/* Item 0 is 12 10 + 16 bytes (slot 0 is omitted), item 1 is 08 01 12 10 + 16 bytes: 0a 12 <18> 0a 14 <20> is 42 bytes. */
#define LIST2_SIZE 42u

static const char *check_calls(void)
{
    static uint8_t out[64];
    static const uint8_t two[] = {0x08, 0x02};
    static const uint8_t cut[] = {0x08, 0x80};
    const dk_workbench_query *q = &dk_workbench_queries[0];
    uint32_t len;
    if (DK_WORKBENCH_QUERY_COUNT != 1 || q->method != 1)
        return "method numbering";
    memset(out, 0xaa, sizeof out);
    len = LIST2_SIZE;
    if (q->call(0, two, sizeof two, out, &len) != 0 || len != LIST2_SIZE)
        return "reply that just fits";
    if (out[0] != 0x0a || out[1] != 0x12 || out[2] != 0x12 || out[3] != 0x10 || out[20] != 0x0a ||
        out[21] != 0x14 || out[LIST2_SIZE] != 0xaa)
        return "reply bytes";
    memset(out, 0xaa, sizeof out);
    len = LIST2_SIZE - 1;
    if (q->call(0, two, sizeof two, out, &len) != DK_E_TOO_BIG)
        return "room too small";
    for (uint32_t i = LIST2_SIZE - 1; i < sizeof out; i++)
        if (out[i] != 0xaa)
            return "write past *out_len";
    len = 0;
    if (q->call(0, two, sizeof two, out, &len) != DK_E_TOO_BIG)
        return "no room";
    len = 855;
    if (q->call(0, cut, sizeof cut, out, &len) != DK_E_FORMAT)
        return "truncated request";
    return 0;
}

int main(void)
{
    static char line[8192];
    static uint8_t in[4096];
    while (fgets(line, sizeof line, stdin)) {
        char name[32], hex[8000];
        uint32_t n = 0;
        hex[0] = 0;
        if (sscanf(line, "%31s %7999s", name, hex) < 1)
            return 2;
        if (!strcmp(name, "@calls")) {
            const char *bad = check_calls();
            if (bad)
                printf("CALLS-FAIL %s\n", bad);
            else
                puts("CALLS-OK");
            continue;
        }
        if (unhex(hex, in, sizeof in, &n) < 0)
            return 2;
        run(name, in, n);
    }
    return 0;
}
