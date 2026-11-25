/* test/local-step-user: imports local-step's export, which is all a provider needs to load. The call logs the answer once at init (the export enters the owner outside a step). */
#include <dk/core/log.h>
#include <dk/core/module.h>

uint32_t local_step_value(void);

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_log(DK_LOG_INFO, "local-step", local_step_value());
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
