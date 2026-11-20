/* test/svc-echo: a module with SysEx commands of its own (docs/design/module-sdk.md, "Service commands").
 *
 * init registers 0x3e and 0x3f, which the remote_state range [0x3e, 0x3f] of sdk/sdk.toml reserves, and checks that the runtime refuses what it must:
 * a range outside every reserved range, one that overlaps its own, and an empty one (each result is kept in `svc_refused`).
 *
 *   0x3e  echo: the reply body is the request body; a body that starts with 0xee answers the status -77 (a status of the module's own, no body)
 *   0x3f  the refusals: three bytes, the low byte of each result of dk_host_register (0xc0 is -64, E_CMD_RANGE)
 */

#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>

typedef uint32_t u32;
typedef uint8_t u8;

u8 svc_refused[3];

static int32_t echo(u32 cmd, u32 seq, const void *request, u32 len, void *reply)
{
    const u8 *body = request;
    u8 *out = reply;
    u32 i;
    (void)cmd;
    (void)seq;
    if (len > 0 && body[0] == 0xee)
        return -77;
    for (i = 0; i < len; i++)
        out[i] = body[i];
    return (int)len;
}

static int32_t refusals(u32 cmd, u32 seq, const void *request, u32 len, void *reply)
{
    u8 *out = reply;
    (void)cmd;
    (void)seq;
    (void)request;
    (void)len;
    out[0] = svc_refused[0];
    out[1] = svc_refused[1];
    out[2] = svc_refused[2];
    return 3;
}

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    int ok = dk_host_register(0x3e, 1, echo);
    dk_host_register(0x3f, 1, refusals);
    svc_refused[0] = (u8)dk_host_register(0x10, 1, echo); /* outside every reserved range */
    svc_refused[1] = (u8)dk_host_register(0x3e, 1, echo); /* over a registered one */
    svc_refused[2] = (u8)dk_host_register(0x38, 0, echo); /* empty */
    dk_log(DK_LOG_INFO, "svc-echo registered", (u32)ok);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
