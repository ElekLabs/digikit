/* RAM-only diagnostic: stock setup/task/bitmap ownership and completion stay intact. */
#include <stdint.h>
#include <dk_fw_draw.h>

volatile uint32_t BOOT_SCENE_PUBLISHED __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_CALLS __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_LAST_INDEX __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_SEEN __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_ERRORS __attribute__((aligned(4)));

static void diagnostic(uint32_t index, uint32_t count, uint8_t *bitmap)
{
    if (count != 3 || index >= 3 || index != BOOT_SCENE_CALLS)
        BOOT_SCENE_ERRORS++;
    BOOT_SCENE_CALLS++;
    BOOT_SCENE_LAST_INDEX = index;
    if (index >= 3)
        return;
    BOOT_SCENE_SEEN |= 1u << index;
    dk_fw_draw_fill_rect(bitmap, 0, 0, 127, 63, 0);
    dk_fw_draw_fill_rect(bitmap, 0, 0, 5, 2, -1);
    dk_fw_draw_fill_rect(bitmap, 123, 61, 127, 63, -1);
    dk_fw_draw_fill_rect(bitmap, 17, 11, 18, 18, -1);
    dk_fw_draw_fill_rect(bitmap, 32 + 8 * index, 32, 35 + 8 * index, 39, -1);
}

typedef struct {
    void (*callback)(uint32_t, uint32_t, uint8_t *);
    uint32_t count;
} Scene;
static const Scene scenes[] __attribute__((aligned(4))) = {
    { diagnostic, 3 },
    { 0, 0 },
};

void dk_boot_early(void)
{
    /* Existing boot guard calls here only after persistence and exception-hook verification. */
    BOOT_SCENE_PUBLISHED = (uint32_t)(uintptr_t)scenes;
}
