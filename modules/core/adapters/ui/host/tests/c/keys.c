/* Host test of the reverse key lookup of core/ui (src/key_map.h, the same source ui.c links). */
#include <stdint.h>
#include "../../../src/key_map.h"

/* codes: the key map as the device file gives it (index = logical key); n entries. */
uint32_t km_lookup(const uint32_t *codes, uint32_t n, uint32_t code)
{
    return ui_key_from_codes(codes, n, code);
}
