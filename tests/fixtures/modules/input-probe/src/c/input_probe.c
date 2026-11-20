/* test/input-probe: SysEx commands that press keys and turn encoders through core/input (docs/plans/remote-control.md). The proof of the input adapter:
 * what these commands do is compared with the emulator's own input.button and input.turn. digikit/remote is the module for players.
 *
 *   0x38  key:  body = code (u8), down (u8)       reply: status 0, or core/input's negative result
 *   0x39  turn: body = encoder (u8), delta (i8)   reply: status 0, or the negative result
 *
 * dk_input_open gives the handle in init; a runtime without core/input answers DK_E_MISSING and this module does not load (it has nothing else to do). */

#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/platform/host.h>
#include <dk/platform/input.h>

typedef uint32_t u32;
typedef uint8_t u8;
typedef int8_t s8;

static dk_input *input;

static int32_t key(u32 cmd, u32 seq, const void *request, u32 len, void *reply)
{
    const u8 *body = request;
    (void)cmd;
    (void)seq;
    (void)reply;
    if (len != 2u)
        return -50; /* E_MSG */
    return dk_input_key(input, body[0], body[1]);
}

static int32_t turn(u32 cmd, u32 seq, const void *request, u32 len, void *reply)
{
    const u8 *body = request;
    (void)cmd;
    (void)seq;
    (void)reply;
    if (len != 2u)
        return -50;
    return dk_input_turn(input, body[0], (s8)body[1]);
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_input_open(ctx, &input);
    if (r != DK_OK)
        return r;
    int a = dk_host_register(0x38, 1, key);
    int b = dk_host_register(0x39, 1, turn);
    dk_log(DK_LOG_INFO, "input-probe registered", (u32)(a | b));
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
