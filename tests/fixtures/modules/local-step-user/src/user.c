/* test/local-step-user: imports local-step's export, which is all a provider needs to load. It never calls it: the test counts the calls of the hook's steps. */
#include <dk/core/log.h>
#include <dk/core/module.h>

uint32_t local_step_value(void);

/* the reference that makes the import real */
static uint32_t (*volatile imported)(void) = local_step_value;

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_log(DK_LOG_INFO, "local-step", imported != 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
