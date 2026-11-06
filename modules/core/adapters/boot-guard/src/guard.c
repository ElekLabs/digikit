/* Private linked callback proof: no asset I/O, loader phase or SDK service contract. */
#include <dkrt.h>

extern unsigned int dkrt_boot_enter_early(void);
extern unsigned int dkrt_boot_settings_loaded(void);

/* Opt-in proof instrumentation, not the cost of a future player. A before snapshot exists only after the
 * counter write returned OK and the actual exception-hook word was verified. No extra card I/O here. */
volatile unsigned int BOOT_GUARD_ATTEMPTS;
volatile unsigned int BOOT_GUARD_ENTERED;
volatile unsigned int BOOT_GUARD_RETURNED;
volatile unsigned int BOOT_GUARD_REQUESTS;
volatile unsigned int BOOT_GUARD_SETTINGS_BEFORE;
volatile unsigned int BOOT_GUARD_SETTINGS_AFTER;
/* Aligned single-word observations: bit 0 received channel 2, bit 1 ever saw RECORD down.
 * DECISION is captured once at intro; later/in-flight observer stores cannot change it. */
volatile unsigned int BOOT_GUARD_PANEL_BITS __attribute__((aligned(4)));
volatile unsigned int BOOT_GUARD_CLOSED __attribute__((aligned(4)));
volatile unsigned int BOOT_GUARD_DECISION __attribute__((aligned(4)));
DkrtBootInfo BOOT_GUARD_BEFORE;
DkrtBootInfo BOOT_GUARD_AFTER;

void boot_guard_observe(unsigned int channel, unsigned int mask)
{
    if (channel == 2 && !BOOT_GUARD_CLOSED)
        BOOT_GUARD_PANEL_BITS |= 1u | ((mask & 4u) ? 2u : 0u);
}

void boot_guard_enter(void (*callback)(void))
{
    BOOT_GUARD_ATTEMPTS++;
    if (BOOT_GUARD_DECISION != 1u)
        return;
    BOOT_GUARD_REQUESTS++;
    BOOT_GUARD_SETTINGS_BEFORE = dkrt_boot_settings_loaded();
    unsigned int permitted = dkrt_boot_enter_early();
    BOOT_GUARD_SETTINGS_AFTER = dkrt_boot_settings_loaded();
    if (!permitted)
        return;
    dkrt_boot_info(&BOOT_GUARD_BEFORE);
    BOOT_GUARD_ENTERED++;
    callback();
    BOOT_GUARD_RETURNED++;
    dkrt_boot_info(&BOOT_GUARD_AFTER);
}
