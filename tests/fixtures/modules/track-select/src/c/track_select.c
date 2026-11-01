/* test/track-select: the test-only route to `sample_select` for a module that owns a track table (digikit/spark has no picker).
 * It owns no table. The runtime does not check which module a handle belongs to, and the first module with a table gets handle 1
 * (client slot 0), so the test passes the handle in the request. Not a product picker. Commands (the remote_state range of sdk/sdk.toml):
 *
 *   0x3e  select:  body = handle, track, then the path bytes (no NUL; at most 159); the reply status is the result of sample_select
 *                  (0 = queued, else the error code), no reply body
 *   0x3f  status:  body = handle, track; the reply body is the status of sample_track_status as a signed BE32 (0 unused, 1 queued,
 *                  2 loading, 3 ready, < 0 error) and, when the track is ready, the 16 bytes of sample_track_info (BE32 frames,
 *                  rate, channels, token)
 * Both run in the MIDI task: they only change state, the runtime loads in its UI loop.
 */
#include <dk/api.h>
#include <dk/sample_track.h>

typedef uint32_t u32;
typedef uint8_t u8;

static const dk_api *runtime;

static int32_t select_cmd(u32 cmd, u32 seq, const u8 *body, u32 len, u8 *out)
{
    u8 path[160];
    u32 i;
    (void)cmd;
    (void)seq;
    (void)out;
    if (len < 3 || len > 2 + 159)
        return DK_SAMPLE_E_FORMAT;
    for (i = 2; i < len; i++)
        path[i - 2] = body[i];
    path[len - 2] = 0;
    return runtime->sample_select(body[0], body[1], path);
}

static int32_t status_cmd(u32 cmd, u32 seq, const u8 *body, u32 len, u8 *out)
{
    int32_t status;
    (void)cmd;
    (void)seq;
    if (len != 2)
        return DK_SAMPLE_E_FORMAT;
    status = runtime->sample_track_status(body[0], body[1]);
    out[0] = (u8)((u32)status >> 24);
    out[1] = (u8)((u32)status >> 16);
    out[2] = (u8)((u32)status >> 8);
    out[3] = (u8)status;
    if (status != DK_SAMPLE_READY)
        return 4;
    if (runtime->sample_track_info(body[0], body[1], out + 4) != 0)
        return 4;
    return 20;
}

int32_t dkm_init(const dk_api *api)
{
    if (api->version != 1 || api->size < offsetof(dk_api, sample_track_info) + sizeof(api->sample_track_info)
        || !api->sample_select || !api->sample_track_status || !api->sample_track_info)
        return DK_SAMPLE_E_CONTRACT;
    runtime = api;
    api->register_cmds(0x3e, 1, select_cmd);
    api->register_cmds(0x3f, 1, status_cmd);
    return 0;
}
