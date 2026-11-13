/* test/dyn-crash: a module that raises a CPU exception some time into a session (docs/findings/46-digikit-debug.md).
 *
 * The delay and the kind come from the store file `cfg/crash-after.bin` (5 bytes: u32 big endian blocks, then 0 = illegal instruction,
 * 1 = division by zero); no file: never crashes. Just before it does, it logs `crash-pc` with the address of the illegal instruction (mode 0) or
 * of the divide (mode 1), which is the program counter the crash record must show. (A read of an unmapped address is no good as a second
 * kind: the emulator reports it as a bus fault of the run, not as an exception the guest can handle.) */

#include <dk/core/log.h>
#include <dk/core/module.h>
#include <dk/core/time.h>
#include <dk/platform/files.h>

typedef uint32_t u32;

static u32 after = 0xffffffffu;
static u32 mode;
static u32 start;
static u32 started;

static int32_t init(dk_module_ctx *ctx)
{
    unsigned char cfg[16];
    dk_files *files;
    int32_t r = dk_files_open(ctx, &files);
    if (r != DK_OK)
        return r;
    int n = dk_files_read(files, "cfg/crash-after.bin", cfg, sizeof cfg);
    if (n >= 5) {
        after = ((u32)cfg[0] << 24) | ((u32)cfg[1] << 16) | ((u32)cfg[2] << 8) | cfg[3];
        mode = cfg[4];
    }
    dk_log(DK_LOG_INFO, "crash-after", after);
    return DK_OK;
}

static void tick(dk_module_ctx *ctx)
{
    u32 now = dk_time_blocks();
    register u32 pc;
    (void)ctx;
    if (!started) {
        started = 1;
        start = now;
        return;
    }
    if (now - start < after) {
        return;
    }
    if (mode == 0) {
        __asm__ volatile("lea 2f(%%pc),%0" : "=a"(pc));
        dk_log(DK_LOG_INFO, "crash-pc", pc);
        __asm__ volatile("2: .short 0x4afc");
    } else {
        /* a division by zero; the divisor is read through a volatile so that the compiler cannot see it */
        volatile u32 zero = 0;
        register u32 x = 1234;
        __asm__ volatile("lea 3f(%%pc),%0" : "=a"(pc));
        dk_log(DK_LOG_INFO, "crash-pc", pc);
        __asm__ volatile("3: divu.l %1,%0" : "+d"(x) : "d"(zero));
    }
    for (;;) {
    }
}

const dk_module_def dk_module = {.init = init, .tick = tick};
