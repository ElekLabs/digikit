/* The reverse lookup of the key map (key_map.h). Pure logic. */
#include "key_map.h"

dk_key ui_key_from_codes(const uint32_t *codes, uint32_t n, uint32_t code)
{
    if (code == 0)
        return DK_KEY_NONE;
    for (uint32_t k = 1; k < n; k++)
        if (codes[k] == code)
            return k;
    return DK_KEY_NONE;
}
