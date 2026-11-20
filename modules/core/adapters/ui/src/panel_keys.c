/* The translation of a panel code to a logical key for module views (view_pool.h): the reverse of the device's key map. */
#include "view_pool.h"
#include "key_map.h"
#include "dkrt.h"

extern const DkrtDevice dkrt_dev;

dk_key ui_key_of_code(uint32_t code)
{
    return ui_key_from_codes(dkrt_dev.key_codes, DKRT_DEV_KEYS, code);
}
