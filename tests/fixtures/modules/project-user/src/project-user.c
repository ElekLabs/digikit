/* test/project-user: an ordinary module that knows nothing of projects. `init` opens its settings (`dk_settings_open`) and logs what it reads under its key "seen" (`project-read`: the
 * value as a 32-bit big-endian word, or the status of the get when there is none); `dk_project_status` makes the hard NEED on the `project` service, so the card's service file is
 * loaded with it. The test chooses the value: service command PROJECT_CMD queues a put of its body (1 to 16 bytes) under "seen" and `tick` runs it in the UI task (the settings
 * API is not for the MIDI task, where service commands run), then logs `project-put` with the status. PROJECT_CMD + 1 answers: the result of the get in `init` (i32: the value's length or a
 * status), the number of puts that ran (u32), the result of the last put (i32) and the first 16 bytes of the value `init` read. With the service loaded the value is per project (the runtime's overlay), without it the module's settings are what
 * they always were. Browse: PROJECT_CMD + 2 (body: the slot, u32) queues a call of `dk_project_list` and `dk_project_get` for the slot in the UI task (both run there), and `tick` repeats the get while it answers DK_E_AGAIN;
 * PROJECT_CMD + 3 answers the list result (i32), the get result (i32), the first BROWSE_MAX `dk_project_entry` and the raw `dk_project_summary`, so a test sees what the exports gave. */
#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/project.h>
#include <dk/platform/settings.h>

#define PROJECT_CMD 0x38
#define VAL 16u
#define BROWSE_MAX 8u

static dk_settings *settings;
static uint8_t req[VAL];
static uint8_t req_len;
static int32_t init_rc, put_rc;
static uint32_t puts;
static uint8_t init_val[VAL];
static volatile uint32_t pending;
static volatile uint32_t browse_pending;
static uint32_t browse_slot;
static int32_t list_rc, get_rc = DK_E_AGAIN;
static dk_project_entry entries[BROWSE_MAX];
static dk_project_summary summary;
static volatile uint32_t browse_done;

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
    if (cmd == PROJECT_CMD + 2) {
        if (len != 4 || browse_pending)
            return -50;
        browse_slot = dk_bytes_get32(body);
        browse_done = 0;
        __asm__ volatile("" ::: "memory");
        browse_pending = 1;
        return 0;
    }
    if (cmd == PROJECT_CMD + 3) {
        if (!browse_done)
            return -51;
        dk_bytes_put32(out, (uint32_t)list_rc);
        dk_bytes_put32(out + 4, (uint32_t)get_rc);
        const uint8_t *e = (const uint8_t *)entries;
        for (uint32_t i = 0; i < sizeof entries; ++i)
            out[8 + i] = e[i];
        const uint8_t *s = (const uint8_t *)&summary;
        for (uint32_t i = 0; i < sizeof summary; ++i)
            out[8 + sizeof entries + i] = s[i];
        return (int32_t)(8 + sizeof entries + sizeof summary);
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
    return dk_host_register(PROJECT_CMD, 4, command);
}

static void tick(dk_module_ctx *ctx)
{
    (void)ctx;
    if (browse_pending) {
        list_rc = dk_project_list(0, entries, BROWSE_MAX);
        get_rc = dk_project_get(browse_slot, &summary);
        if (get_rc != DK_E_AGAIN) {
            browse_done = 1;
            browse_pending = 0;
        }
    }
    if (pending) {
        put_rc = dk_settings_put(settings, "seen", req, req_len);
        puts++;
        __asm__ volatile("" ::: "memory");
        pending = 0;
        dk_log(DK_LOG_INFO, "project-put", (uint32_t)put_rc);
    }
}

const dk_module_def dk_module = {.init = init, .tick = tick};
