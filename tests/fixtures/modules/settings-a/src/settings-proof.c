/* Module settings API fixture (<dk/platform/settings.h>). `init` takes the handle and reads the key "boot"; a service command queues one put, get or remove
   request and `tick` runs it in the UI task (the settings API is not for the MIDI task, where service commands run).
   test/settings-b builds this file with another command number and PROOF_VIA_JOB, so two module names use the same key. */
#include <dk/core/bytes.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/settings.h>
#ifndef PROOF_CMD
#define PROOF_CMD 0x38 /* request; PROOF_CMD + 1 reads the result */
#endif
#define VAL 16u
#define KEYS 64u
static dk_settings *settings;
static int32_t init_rc, result;
static uint8_t init_val[VAL], val[VAL], req_val[VAL];
static char req_key[KEYS];
static uint8_t op, vlen;
static volatile uint32_t pending;
#ifdef PROOF_VIA_JOB
static int32_t job_step(void *arg);
static void job_done(void *arg, int32_t status);
#endif
/* Request: op (1 put, 2 get, 3 remove), key length, the key bytes, the value (at most VAL bytes).
   Reply of PROOF_CMD + 1: pending (u8), the last result (i32), the result of the read in init (i32), its value (VAL bytes), the last get's value (VAL bytes). */
__attribute__((optimize("no-jump-tables"))) static int32_t
command(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    const uint8_t *body = request;
    uint8_t *out = reply;
    (void)seq;
    if (cmd == PROOF_CMD) {
        if (len < 2 || pending)
            return -12;
        uint32_t k = body[1];
        if (k >= KEYS || len < 2 + k || len - 2 - k > VAL)
            return -50;
        op = body[0];
        for (uint32_t i = 0; i < k; ++i)
            req_key[i] = (char)body[2 + i];
        req_key[k] = 0;
        vlen = (uint8_t)(len - 2 - k);
        for (uint32_t i = 0; i < vlen; ++i)
            req_val[i] = body[2 + k + i];
        __asm__ volatile("" ::: "memory");
        pending = 1;
#ifdef PROOF_VIA_JOB
        if (dk_job_post(job_step, job_done, 0) <= 0) {
            pending = 0;
            return -50;
        }
#endif
        return 0;
    }
    if (cmd == PROOF_CMD + 1) {
        out[0] = (uint8_t)pending;
        dk_bytes_put32(out + 1, (uint32_t)result);
        dk_bytes_put32(out + 5, (uint32_t)init_rc);
        for (uint32_t i = 0; i < VAL; ++i) {
            out[9 + i] = init_val[i];
            out[9 + VAL + i] = val[i];
        }
        return (int32_t)(9 + 2 * VAL);
    }
    return -50;
}
static void run_request(void)
{
    if (!pending)
        return;
    if (op == 1)
        result = dk_settings_put(settings, req_key, req_val, vlen);
    else if (op == 2)
        result = dk_settings_get(settings, req_key, val, VAL);
    else if (op == 3)
        result = dk_settings_remove(settings, req_key);
    else
        result = -50;
    __asm__ volatile("" ::: "memory");
    pending = 0;
}
#ifdef PROOF_VIA_JOB
/* This one runs its request in the `done` callback of a job (the UI task), the other path of the same code. */
static int32_t job_step(void *arg)
{
    (void)arg;
    return 0;
}
static void job_done(void *arg, int32_t status)
{
    (void)arg;
    (void)status;
    run_request();
}
#else
static void tick(dk_module_ctx *ctx)
{
    (void)ctx;
    run_request();
}
#endif
static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_settings_open(ctx, &settings);
    if (r != DK_OK)
        return r;
    init_rc = dk_settings_get(settings, "boot", init_val, VAL);
    return dk_host_register(PROOF_CMD, 2, command);
}
#ifdef PROOF_VIA_JOB
const dk_module_def dk_module = {.init = init};
#else
const dk_module_def dk_module = {.init = init, .tick = tick};
#endif
