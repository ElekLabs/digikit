/* test/intro-fault: takes the boot animation and faults in the first frame callback (on the intro task). */

#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/platform/intro.h>

static uint8_t blank[DK_INTRO_BYTES] __attribute__((aligned(4)));

static const uint8_t *frame(uint32_t tick, void *arg)
{
    (void)tick;
    (void)arg;
    __asm__ volatile(".short 0x4afc"); /* illegal instruction */
    return blank;
}

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    int32_t rc = dk_intro_take(frame, 0, 0, 30);
    dk_log(DK_LOG_INFO, "intro-fault: take", (uint32_t)rc);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
