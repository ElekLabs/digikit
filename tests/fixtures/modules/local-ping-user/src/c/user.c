#include <dk/core/log.h>
#include <dk/core/module.h>

uint32_t local_ping_value(void);

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_log(DK_LOG_INFO, "local-ping", local_ping_value());
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
