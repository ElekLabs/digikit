/* Private linked callback proof: no asset I/O, loader phase or SDK service contract. */
#include <dkrt.h>

extern unsigned int dkrt_boot_enter_early(void);

/* Opt-in proof instrumentation, not the cost of a future player. A before snapshot exists only after the
 * counter write returned OK and the actual exception-hook word was verified. No extra card I/O here. */
volatile unsigned int BOOT_GUARD_ATTEMPTS;
volatile unsigned int BOOT_GUARD_ENTERED;
volatile unsigned int BOOT_GUARD_RETURNED;
DkrtBootInfo BOOT_GUARD_BEFORE;
DkrtBootInfo BOOT_GUARD_AFTER;

void boot_guard_enter(void (*callback)(void))
{
    BOOT_GUARD_ATTEMPTS++;
    if (!dkrt_boot_enter_early())
        return;
    dkrt_boot_info(&BOOT_GUARD_BEFORE);
    BOOT_GUARD_ENTERED++;
    callback();
    BOOT_GUARD_RETURNED++;
    dkrt_boot_info(&BOOT_GUARD_AFTER);
}
