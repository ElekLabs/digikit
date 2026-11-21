/* The FX service's v1 query contract for resident image code. */
#ifndef DK_CORE_FX_ROUTE_QUERY_H
#define DK_CORE_FX_ROUTE_QUERY_H

#include <dkrt.h>

typedef struct {
    uint32_t row;           /* DkrtFx.row of the edited effect */
    uint32_t pending_sends; /* One bit per DkrtFx.row whose pending choice is SEND */
} DkFxRouteChoicesV1;

_Static_assert(sizeof(DkFxRouteChoicesV1) == 8, "FX route query v1 layout");

/* The allowed routes as DkrtFx.allowed bits. Written only on success. */
static inline int32_t dk_fx_route_choices(const DkFxRouteChoicesV1 *request, uint32_t *allowed)
{
    return dkrt_service_query(0x4d22c733u, /* name_hash("fx") */
                              0x000803u,   /* FX service 0.8.3 */
                              1,           /* ROUTE_CHOICES_V1 */
                              (const uint8_t *)request, sizeof(*request), (uint8_t *)allowed,
                              sizeof(*allowed));
}

#endif
