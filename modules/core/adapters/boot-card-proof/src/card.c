/* Private boot-only provider. Successful bytes stay immutable until reset. */
#include <stdint.h>
#include <dk/gen/api.h>
#include <dkrt.h>

extern dk_api dkrt_api;

volatile uint32_t BOOT_CARD_ATTEMPTS __attribute__((aligned(4)));
volatile uint32_t BOOT_CARD_STATS __attribute__((aligned(4)));
volatile uint32_t BOOT_CARD_READS __attribute__((aligned(4)));
volatile uint32_t BOOT_CARD_OWNED_BYTES __attribute__((aligned(4)));
volatile uint32_t BOOT_CARD_POINTER __attribute__((aligned(4)));
volatile uint32_t BOOT_CARD_DISCARDS __attribute__((aligned(4)));

static const uint8_t path[] = "bootscreens/active.dkbs";
static uint8_t *owned;
static uint32_t actual_size;

void dk_boot_asset_discard(void)
{
    if (owned) {
        dkrt_api.free(owned);
        owned = 0;
        actual_size = 0;
        BOOT_CARD_OWNED_BYTES = 0;
        BOOT_CARD_POINTER = 0;
        BOOT_CARD_DISCARDS++;
    }
}

const uint8_t *dk_boot_asset(uint32_t *size)
{
    *size = 0;
    /* The early guard calls once. Never retry a failed attempt or allocate twice. */
    if (BOOT_CARD_ATTEMPTS) {
        *size = actual_size;
        return owned;
    }
    BOOT_CARD_ATTEMPTS++;
    DkrtStoreFile file;
    BOOT_CARD_STATS++;
    if (dkrt_store_stat(path, &file) != 0 || file.size < 16 || file.size > 65536)
        return 0;
    owned = dkrt_api.alloc(file.size);
    if (!owned)
        return 0;
    BOOT_CARD_OWNED_BYTES = file.size;
    BOOT_CARD_POINTER = (uint32_t)(uintptr_t)owned;
    BOOT_CARD_READS++;
    int32_t result = dkrt_api.store_read(path, owned, file.size);
    if (result <= 0 || (uint32_t)result > file.size) {
        dk_boot_asset_discard();
        return 0;
    }
    actual_size = (uint32_t)result;
    *size = actual_size;
    return owned;
}
