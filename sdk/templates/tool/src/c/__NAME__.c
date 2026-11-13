/* __ID__: a service module (docs/plans/sdk.md section 5).
 *
 * `init` runs when the runtime loads the module; `fini` when it unloads it. `dk_module` is the module's one entry point: the packer finds it by its symbol.
 *
 * To add SysEx commands, allocate a range in sdk/sdk.toml [protocol.ranges] and register it in `init`:
 *
 *     dk_host_register(FIRST_CMD, COUNT, handler);        (<dk/platform/host.h>)
 *
 * A service with a handle (settings, files, ui, ...) is opened in `init`: `dk_settings_open(ctx, &settings)`.
 */

#include <dk/core/module.h>
#include <dk/core/log.h>

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_log(DK_LOG_INFO, "__NAME__: hello", 0);
    return DK_OK;
}

static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
}

const dk_module_def dk_module = {.init = init, .fini = fini};
