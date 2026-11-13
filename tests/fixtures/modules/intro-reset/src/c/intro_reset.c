/* test/intro-reset: takes the boot animation and resets the machine in the first frame callback (on the intro task): it writes SOFTRST (0x80)
 * into RCM.RCR (0xEC090000), as test/dyn-fault does. A frame callback makes no runtime call. */

#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/platform/intro.h>

static uint8_t blank[DK_INTRO_BYTES] __attribute__((aligned(4)));

static const uint8_t *frame(uint32_t tick, void *arg)
{
    (void)tick;
    (void)arg;
    *(volatile unsigned char *)0xEC090000 = 0x80;
    for (;;) {
    }
    return blank;
}

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    int32_t rc = dk_intro_take(frame, 0, 0, 30);
    dk_log(DK_LOG_INFO, "intro-reset: take", (uint32_t)rc);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
