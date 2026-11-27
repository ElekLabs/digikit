/* test/project-user: an ordinary module that knows nothing of projects. `init` opens its settings (`dk_settings_open`) and logs what it reads under its key "seen" (`project-read`: the
 * value as a 32-bit big-endian word, or the status of the get when there is none); `dk_project_status` makes the hard NEED on the `project` service, so the card's service file is
 * loaded with it. The test chooses the value: service command PROJECT_CMD queues a put of its body (1 to 16 bytes) under "seen" and `tick` runs it in the UI task (the settings
 * API is not for the MIDI task, where service commands run), then logs `project-put` with the status. PROJECT_CMD + 1 answers: the result of the get in `init` (i32: the value's length or a
 * status), the number of puts that ran (u32), the result of the last put (i32) and the first 16 bytes of the value `init` read. With the service loaded the value is per project (the runtime's overlay), without it the module's settings are what
 * they always were.
 * Browse: the Workbench queries of workbench/project_user.proto, List(first_slot) and Get(slot), call `dk_project_list` and `dk_project_get` in the UI task and
 * return what the exports gave (Get answers loading while the device reads the project's file; `next` of List is always 0).
 */
#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/project.h>
#include <dk/platform/settings.h>
#include <dk/core/status.h>

#include "workbench_descriptor.h"
#include "workbench_queries.h"

#define PROJECT_CMD 0x38
#define VAL 16u
#define PAGE_SIZE 16

static dk_settings *settings;
static uint8_t req[VAL];
static uint8_t req_len;
static int32_t init_rc, put_rc;
static uint32_t puts;
static uint8_t init_val[VAL];
static volatile uint32_t pending;

/* A name as the firmware lists it (zero padded) copied to a proto string. */
static uint32_t text(char *out, uint32_t capacity, const uint8_t *bytes, uint32_t len)
{
    uint32_t n = 0;
    while (n < len && n < capacity - 1 && bytes[n]) {
        out[n] = (char)bytes[n];
        n++;
    }
    out[n] = 0;
    return n;
}

int32_t project_user_list(dk_module_ctx *ctx, const project_user_ListRequest *request,
                          project_user_ListReply *reply)
{
    (void)ctx;
    dk_project_entry entries[PAGE_SIZE];
    int32_t n = dk_project_list(request->first_slot, entries, PAGE_SIZE);
    if (n < 0)
        return n;
    for (int32_t i = 0; i < n; i++) {
        project_user_Project *item = &reply->items[i];
        item->slot = entries[i].slot;
        item->name_len = text(item->name, sizeof item->name, entries[i].name, entries[i].name_len);
        item->current = (entries[i].flags & DK_PROJECT_CURRENT) != 0;
        item->has_data = (entries[i].flags & DK_PROJECT_DATA) != 0;
        item->data_unknown = (entries[i].flags & DK_PROJECT_DATA_UNKNOWN) != 0;
    }
    reply->items_count = (uint32_t)n;
    reply->next = 0;
    return 0;
}

int32_t project_user_get(dk_module_ctx *ctx, const project_user_GetRequest *request,
                         project_user_GetReply *reply)
{
    (void)ctx;
    dk_project_summary summary;
    int32_t r = dk_project_get(request->slot, &summary);
    if (r == DK_E_AGAIN) {
        reply->loading = 1;
        return 0;
    }
    if (r == DK_E_NOT_FOUND)
        return 0;
    if (r != 0)
        return r;
    reply->found = 1;
    reply->slot = summary.slot;
    reply->name_len = text(reply->name, sizeof reply->name, summary.name, summary.name_len);
    reply->stamp = summary.stamp;
    reply->counter = summary.counter;
    uint32_t count = summary.module_count < DK_PROJECT_MODULES_MAX ? summary.module_count
                                                                   : DK_PROJECT_MODULES_MAX;
    for (uint32_t i = 0; i < count; i++) {
        project_user_Module *out = &reply->modules[i];
        out->id_len =
            text(out->id, sizeof out->id, summary.modules[i].id, summary.modules[i].id_len);
        out->hash = summary.modules[i].hash;
        out->settings = summary.modules[i].settings;
        out->loaded = summary.modules[i].state == DK_PROJECT_MODULE_LOADED;
    }
    reply->modules_count = count;
    return 0;
}

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
    r = dk_workbench_register(ctx, dk_workbench_descriptor, sizeof dk_workbench_descriptor);
    if (r != DK_OK)
        return r;
    r = dk_workbench_register_queries(ctx, dk_workbench_queries, DK_WORKBENCH_QUERY_COUNT);
    if (r != DK_OK)
        return r;
    dk_log(DK_LOG_INFO, "project-read",
           init_rc >= 4 ? dk_bytes_get32(init_val) : (uint32_t)init_rc);
    dk_log(DK_LOG_INFO, "project-status", dk_project_status());
    return dk_host_register(PROJECT_CMD, 4, command);
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
