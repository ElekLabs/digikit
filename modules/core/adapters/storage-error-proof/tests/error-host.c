#include <assert.h>
#include <stdint.h>
#include <stdlib.h>

const uint32_t dkrt_store_first = 100, dkrt_store_sectors = 20;
uint32_t BOOT_IO_MODE;
volatile uint32_t BOOT_GUARD_ATTEMPTS;
uint32_t boot_io_decide(uint32_t write, uint32_t sector, uint32_t bytes);
extern volatile uint32_t BOOT_IO_MATCH_READS, BOOT_IO_MATCH_WRITES;
extern volatile uint32_t BOOT_IO_REJECT_READS, BOOT_IO_REJECT_WRITES;
extern volatile uint32_t BOOT_IO_LAST_SECTOR, BOOT_IO_LAST_BYTES, BOOT_IO_LAST_OP;
int main(int argc, char **argv)
{
    assert(argc == 2);
    BOOT_IO_MODE = (uint32_t)atoi(argv[1]);
    assert(!boot_io_decide(0, 119, 512));
    assert(!boot_io_decide(0, 103, 2560));
    assert(!boot_io_decide(1, 100, 512));
    assert(BOOT_IO_MATCH_READS == 0 && BOOT_IO_MATCH_WRITES == 0);
    BOOT_GUARD_ATTEMPTS = 1;
    assert(!boot_io_decide(0, 99, 512));
    assert(!boot_io_decide(1, 120, 512));
    assert(BOOT_IO_MATCH_READS == 0 && BOOT_IO_MATCH_WRITES == 0);
    assert(!boot_io_decide(0, 119, 256));
    assert(!boot_io_decide(0, 102, 5120));
    assert(!boot_io_decide(0, 102, 512));
    uint32_t expected_reads = 0, expected_writes = 0;
    for (uint32_t repeat = 0; repeat < 2; repeat++) {
        for (uint32_t sector = 100; sector < 120; sector++) {
            uint32_t expected = ((BOOT_IO_MODE == 6 && sector == 119) ||
                (BOOT_IO_MODE == 7 && sector >= 118)) || (!repeat && ((BOOT_IO_MODE == 1 && sector == 119) ||
                (BOOT_IO_MODE == 2 && sector >= 118) || (BOOT_IO_MODE == 5 && sector == 100)));
            assert(boot_io_decide(0, sector, 512) == expected);
            expected_reads += expected;
            assert(boot_io_decide(1, sector, 512) == (BOOT_IO_MODE == 4));
            expected_writes += BOOT_IO_MODE == 4;
        }
        uint32_t expected = BOOT_IO_MODE == 3 && !repeat;
        assert(boot_io_decide(0, 103, 2560) == expected);
        expected_reads += expected;
    }
    assert(BOOT_IO_MATCH_READS == 45 && BOOT_IO_MATCH_WRITES == 40);
    assert(BOOT_IO_REJECT_READS == expected_reads && BOOT_IO_REJECT_WRITES == expected_writes);
    if (BOOT_IO_MODE) {
        assert(BOOT_IO_LAST_OP == (BOOT_IO_MODE == 4));
        assert(BOOT_IO_LAST_BYTES == (BOOT_IO_MODE == 3 ? 2560 : 512));
        assert(BOOT_IO_LAST_SECTOR == (BOOT_IO_MODE == 3 ? 103 : BOOT_IO_MODE == 5 ? 100 : 119));
    }
    return 0;
}
