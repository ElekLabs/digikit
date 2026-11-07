/* Test-only returned-driver-error model; never calls storage or touches buffers. */
#include <stdint.h>

extern const uint32_t dkrt_store_first, dkrt_store_sectors, BOOT_IO_MODE;
extern volatile uint32_t BOOT_GUARD_ATTEMPTS;
volatile uint32_t BOOT_IO_MATCH_READS __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_MATCH_WRITES __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_REJECT_READS __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_REJECT_WRITES __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_LAST_SECTOR __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_LAST_BYTES __attribute__((aligned(4)));
volatile uint32_t BOOT_IO_LAST_OP __attribute__((aligned(4)));
static uint32_t used;

uint32_t boot_io_decide(uint32_t write, uint32_t sector, uint32_t bytes)
{
    const uint32_t first = dkrt_store_first, sectors = dkrt_store_sectors;
    uint32_t bit = 0;
    if (!BOOT_GUARD_ATTEMPTS) return 0;
    if (sector < first || sector - first >= sectors) return 0;
    if (write) BOOT_IO_MATCH_WRITES++;
    else BOOT_IO_MATCH_READS++;
    if (BOOT_IO_MODE == 4 && write) bit = 4;
    if (!write) {
        const uint32_t relative = sector - first;
        if (bytes == 512 && sectors >= 2) {
            if ((BOOT_IO_MODE == 1 || BOOT_IO_MODE == 2) && relative == sectors - 1) bit = 1;
            if (BOOT_IO_MODE == 2 && relative == sectors - 2) bit = 2;
        }
        if (BOOT_IO_MODE == 3 && bytes == 2560) bit = 4;
        if (BOOT_IO_MODE == 5 && bytes == 512 && relative == 0) bit = 4;
    }
    if (!bit || (BOOT_IO_MODE != 4 && (used & bit))) return 0;
    used |= bit;
    if (write) BOOT_IO_REJECT_WRITES++;
    else BOOT_IO_REJECT_READS++;
    BOOT_IO_LAST_SECTOR = sector;
    BOOT_IO_LAST_BYTES = bytes;
    BOOT_IO_LAST_OP = write;
    return 1;
}
