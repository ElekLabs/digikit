#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <dk/gen/api.h>
#include <dkrt.h>

const uint8_t *dk_boot_asset(uint32_t *size);
void dk_boot_asset_discard(void);
extern volatile uint32_t BOOT_CARD_ATTEMPTS, BOOT_CARD_OWNED_BYTES, BOOT_CARD_DISCARDS;
static uint32_t stat_size = 2166, allocations, reads, frees;
static int32_t stat_result, read_result = 2166;
static int fail_alloc;
static uint8_t *block;
static void check_path(const uint8_t *path)
{
    assert(strcmp((const char *)path, "bootscreens/active.dkbs") == 0);
}
int32_t dkrt_store_stat(const uint8_t *path, DkrtStoreFile *file)
{
    check_path(path);
    file->size = stat_size;
    return stat_result;
}
static uint8_t *allocate(uint32_t size)
{
    assert(size == stat_size);
    allocations++;
    if (fail_alloc) return 0;
    block = malloc(size);
    assert(block);
    return block;
}
static void release(uint8_t *p)
{
    assert(p == block);
    frees++;
    free(p);
    block = 0;
}
static int32_t read_file(const uint8_t *path, uint8_t *dst, uint32_t cap)
{
    check_path(path);
    assert(dst == block && cap == stat_size);
    reads++;
    memset(dst, 0x5a, cap);
    return read_result;
}
dk_api dkrt_api = { .alloc = allocate, .free = release, .store_read = read_file };
int main(int argc, char **argv)
{
    assert(argc == 2);
    const char *mode = argv[1];
    if (!strcmp(mode, "missing")) stat_result = -1;
    if (!strcmp(mode, "short")) stat_size = 15;
    if (!strcmp(mode, "oversized")) stat_size = 65537;
    if (!strcmp(mode, "allocation")) fail_alloc = 1;
    if (!strcmp(mode, "negative")) read_result = -1;
    if (!strcmp(mode, "zero")) read_result = 0;
    if (!strcmp(mode, "overread")) read_result = 2167;
    if (!strcmp(mode, "actual")) read_result = 2165;
    uint32_t size = 0xffffffffu;
    const uint8_t *asset = dk_boot_asset(&size);
    int success = !strcmp(mode, "valid") || !strcmp(mode, "actual");
    int stat_denied = stat_result || stat_size < 16 || stat_size > 65536;
    assert(allocations == (stat_denied ? 0u : 1u));
    assert(reads == (stat_denied || fail_alloc ? 0u : 1u));
    assert(BOOT_CARD_ATTEMPTS == 1);
    if (success) {
        assert(asset == block && size == (uint32_t)read_result && frees == 0);
        assert(BOOT_CARD_OWNED_BYTES == stat_size);
        for (uint32_t i = 0; i < size; i++) assert(asset[i] == 0x5a);
        uint32_t again;
        assert(dk_boot_asset(&again) == asset && again == size);
        assert(allocations == 1 && reads == 1 && frees == 0);
    } else {
        assert(!asset && !size && !BOOT_CARD_OWNED_BYTES);
        assert(frees == reads);
    }
    dk_boot_asset_discard();
    dk_boot_asset_discard();
    assert(!BOOT_CARD_OWNED_BYTES && !block);
    assert(BOOT_CARD_DISCARDS == frees);
    uint32_t again = 99;
    assert(!dk_boot_asset(&again) && !again);
    assert(allocations == (stat_denied ? 0u : 1u));
    return 0;
}
