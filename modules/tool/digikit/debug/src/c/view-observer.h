#ifndef DK_DEBUG_VIEW_OBSERVER_H
#define DK_DEBUG_VIEW_OBSERVER_H

#include <stdint.h>

int32_t debug_view_observer_config(const uint8_t *body, uint32_t len, uint8_t *out);
void debug_view_observer_tick(void);

#endif
