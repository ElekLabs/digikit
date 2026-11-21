/* test/project-user: an ordinary module that knows nothing of projects. `init` opens its settings (`dk_settings_open`) and logs what it reads under its key "seen" (`project-read`: the
 * value as a 32-bit big-endian word, or the status of the get when there is none); `dk_project_status` makes the hard NEED on the `project` service, so the card's service file is
 * loaded with it. The test chooses the value: service command PROJECT_CMD queues a put of its body (1 to 16 bytes) under "seen" and `tick` runs it in the UI task (the settings
 * API is not for the MIDI task, where service commands run), then logs `project-put` with the status. PROJECT_CMD + 1 answers: the result of the get in `init` (i32: the value's length or a
 * status), the number of puts that ran (u32), the result of the last put (i32) and the first 16 bytes of the value `init` read. With the service loaded the value is per project (the runtime's overlay), without it the module's settings are what
 * they always were. */
#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/settings.h>

#define PROJECT_CMD 0x38
#define VAL 16u

static dk_settings *settings;
static uint8_t req[VAL];
static uint8_t req_len;
static int32_t init_rc, put_rc;
static uint32_t puts;
static uint8_t init_val[VAL];
static volatile uint32_t pending;

static int32_t command(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    const uint8_t *body = request;
    uint8_t *out = reply;
    (void)seq;
    if (cmd == PROJECT_CMD + 1) {
        dk_bytes_put32(out, (uint32_t)init_rc);
        dk_bytes_put32(out + 4, puts);
        dk_bytes_put32(out + 8, (uint32_t)put_rc);
        for (uint32_t i = 0; i < VAL; ++i)
            out[12 + i] = init_val[i];
        dk_bytes_put32(out + 12 + VAL, dk_project_status());
        return (int32_t)(16 + VAL);
    }
    if (len < 1 || len > VAL || pending)
        return -50;
    for (uint32_t i = 0; i < len; ++i)
        req[i] = body[i];
    req_len = (uint8_t)len;
    __asm__ volatile("" ::: "memory");
    pending = 1;
    return 0;
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_settings_open(ctx, &settings);
    if (r != DK_OK)
        return r;
    init_rc = dk_settings_get(settings, "seen", init_val, sizeof init_val);
    dk_log(DK_LOG_INFO, "project-read",
           init_rc >= 4 ? dk_bytes_get32(init_val) : (uint32_t)init_rc);
    dk_log(DK_LOG_INFO, "project-status", dk_project_status());
    return dk_host_register(PROJECT_CMD, 2, command);
}

static void tick(dk_module_ctx *ctx)
{
    (void)ctx;
    if (pending) {
        put_rc = dk_settings_put(settings, "seen", req, req_len);
        puts++;
        __asm__ volatile("" ::: "memory");
        pending = 0;
        dk_log(DK_LOG_INFO, "project-put", (uint32_t)put_rc);
    }
}

const dk_module_def dk_module = {.init = init, .tick = tick};
