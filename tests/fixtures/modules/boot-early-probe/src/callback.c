/* A linked callback, never loaded from executable card content. */
volatile unsigned int BOOT_EARLY_CALLS;

void dk_boot_early(void)
{
    BOOT_EARLY_CALLS++;
}
