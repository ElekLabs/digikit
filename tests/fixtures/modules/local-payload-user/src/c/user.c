#include <dk/core/module.h>

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
