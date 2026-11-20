/* The translation of a panel code to a logical key for module views (view_pool.h). */
#include "view_pool.h"

/* TODO(track-manager B1, branch feat/track-manager, sdk/include/dk/gen/keys.h): map the panel code of the firmware's key event (KeyEvent +0xc) to
   the logical key through the device's `[panel]` table. Until that table exists no code has a name, so a full-screen view gets no key. */
uint32_t ui_key_of_code(uint32_t code)
{
    (void)code;
    return UI_KEY_NONE;
}
