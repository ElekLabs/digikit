/* Test-only service: upload first, then 0x3e queues the fixed source path;
   0x3f returns one signed BE32 status word. No file IO in the MIDI callback. It uses the sample bank (dk_sample_bank_*), which S3 deletes. */
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/machine/machine.h>
#include <dk/sample/bank.h>

DK_MACHINE(sample_proof, .name = "Sample proof", .short_name = "SMP", .render = "_sp_render",
           .note_on = "_sp_note_on", .desc = "_sp_desc");
/* Explicit frame bounds for the short proof fixture, latched at note-on.
   END=127 selects the file end; defaults select the complete upload. */
DK_PARAM(sample_proof, 0, start, "Start", "START", 0, 127, 0);
DK_PARAM(sample_proof, 1, end, "End", "END", 0, 127, 127);
DK_PARAM(sample_proof, 2, reverse, "Reverse", "REV", 0, 127, 0);
DK_PAGE(sample_proof, "SAMPLE", "Sample proof", DK_KNOB(0), DK_KNOB(1), DK_KNOB(2));
DK_SAMPLE_BANK(0, _sp_bank, _sp_view, 8192);

static dk_module_ctx *module_ctx;
static dk_sample_bank *bank;

static int32_t command(uint32_t cmd, uint32_t seq, const void *body, uint32_t len, void *reply)
{
    uint8_t *out = reply;
    int32_t result;
    (void)seq;
    (void)body;
    if (len != 0)
        return DK_SAMPLE_E_FORMAT;
    if (cmd == 0x3e) {
        const char *path =
            dk_module_ctx_device(module_ctx) == 1 ? "/proof/test" : "samples/proof/test";
        return dk_sample_bank_load(bank, path);
    }
    result = dk_sample_bank_status(bank);
    out[0] = (uint8_t)((uint32_t)result >> 24);
    out[1] = (uint8_t)((uint32_t)result >> 16);
    out[2] = (uint8_t)((uint32_t)result >> 8);
    out[3] = (uint8_t)result;
    return 4;
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_sample_bank_open(ctx, &bank);
    if (r != DK_OK)
        return r;
    module_ctx = ctx;
    return dk_host_register(0x3e, 2, command);
}

const dk_module_def dk_module = {.init = init};
