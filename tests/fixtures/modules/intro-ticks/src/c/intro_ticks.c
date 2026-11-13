/* test/intro-ticks: takes the boot animation for TICKS ticks. The frame callback (intro task, no runtime call) folds each tick into
 * h = h * 31 + tick + 1 and counts the calls. `done` (UI task, first pass) logs the count and the hash, so a test can prove that every tick
 * ran once and in order: a skipped, repeated or swapped tick changes the hash. */

#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/platform/intro.h>

#define TICKS 40u

static uint8_t blank[DK_INTRO_BYTES] __attribute__((aligned(4)));
static volatile uint32_t hash, calls;

static const uint8_t *frame(uint32_t tick, void *arg)
{
    (void)arg;
    hash = hash * 31u + tick + 1u;
    calls = calls + 1u;
    return blank;
}

static void done(void *arg)
{
    (void)arg;
    dk_log(DK_LOG_INFO, "ticks", calls);
    dk_log(DK_LOG_INFO, "hash", hash);
}

static int32_t init(dk_module_ctx *ctx)
{
    (void)ctx;
    int32_t rc = dk_intro_take(frame, done, 0, TICKS);
    dk_log(DK_LOG_INFO, "intro-ticks: take", (uint32_t)rc);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
