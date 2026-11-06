/* Private linked RAM asset proof; original setup/task/bitmap ownership stay stock. */
#include <stddef.h>
#include <stdint.h>
#include <dk_fw_draw.h>
#include "asset.h"

volatile uint32_t BOOT_SCENE_PUBLISHED __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_CALLS __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_LAST_INDEX __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_ERRORS __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_STATUS __attribute__((aligned(4))); /* 0 absent/denied, 1 valid, 2 invalid */
volatile uint32_t BOOT_SCENE_VALIDATIONS __attribute__((aligned(4)));
volatile uint32_t BOOT_SCENE_DECODED_RECORDS __attribute__((aligned(4)));

static Dkbs asset;
static uint32_t mask[256] __attribute__((aligned(4)));

typedef struct {
    uint32_t vptr, width, height, bands;
    const uint8_t *pixels;
    const uint32_t *mask;
    uint8_t owns, padding[3];
} BitmapView;
_Static_assert(sizeof(void *) == 4, "32-bit target required");
_Static_assert(offsetof(BitmapView, pixels) == 16, "pixels ABI");
_Static_assert(offsetof(BitmapView, mask) == 20, "mask ABI");
_Static_assert(offsetof(BitmapView, owns) == 24, "ownership ABI");
_Static_assert(sizeof(BitmapView) == 28, "Bitmap view ABI");

static void draw(uint32_t index, uint32_t count, uint8_t *bitmap)
{
    if (count != asset.total_ticks || index >= count || index != BOOT_SCENE_CALLS) {
        BOOT_SCENE_ERRORS++;
        return;
    }
    BOOT_SCENE_CALLS++;
    BOOT_SCENE_LAST_INDEX = index;
    if (dkbs_next(&asset) != 1) {
        BOOT_SCENE_ERRORS++;
        return;
    }
    BOOT_SCENE_DECODED_RECORDS = asset.decoded_records;
    /* Explicit volatile stores avoid a compiler-generated memcpy/header template. */
    volatile BitmapView source;
    source.vptr = 0;
    source.width = 128;
    source.height = 64;
    source.bands = 2;
    source.pixels = asset.pixels;
    source.mask = mask;
    source.owns = 0;
    source.padding[0] = 0;
    source.padding[1] = 0;
    source.padding[2] = 0;
    dk_fw_draw_blit(bitmap, (uint8_t *)&source, 0, 0, 0);
}

typedef struct {
    void (*callback)(uint32_t, uint32_t, uint8_t *);
    uint32_t count;
} Scene;
static Scene scenes[] __attribute__((aligned(4))) = { { draw, 0 }, { 0, 0 } };

void boot_scene_prepare(const uint8_t *(*get_asset)(uint32_t *))
{
    uint32_t size = 0;
    const uint8_t *data = get_asset(&size);
    BOOT_SCENE_VALIDATIONS++;
    if (!dkbs_open(&asset, data, size)) {
        BOOT_SCENE_STATUS = 2;
        return;
    }
    /* Existing early guard calls prepare once; no embedded asset or extra card I/O. */
    for (uint32_t i = 0; i < 256; i++)
        mask[i] = 0xffffffffu;
    scenes[0].count = asset.total_ticks;
    BOOT_SCENE_STATUS = 1;
    BOOT_SCENE_PUBLISHED = (uint32_t)(uintptr_t)scenes; /* publish last */
}
