/* core/sample-pool on the Digitone II: what pool.c (the plug-in symbols and the block table) and xfer.c (the page transport) share. */
#ifndef SP_XFER_H
#define SP_XFER_H
#include <stdint.h>
#include "dkrt.h"
#include "dkrt_pool.h"
#include "cf/pool_alloc.h"
#include "cf/pool_wire.h"

#define SP_REQS 16u

/* request states */
#define RQ_FREE 0u
#define RQ_QUEUED 1u   /* waits for the link */
#define RQ_ACTIVE 2u   /* its pages are being sent */

typedef struct SpReq {
    uint32_t state;
    uint32_t req;         /* the runtime's request number */
    uint32_t blk;         /* index in sp_pool */
    uint32_t age;         /* order of arrival */
    uint32_t cancelled;   /* dkpool_cancel: nobody waits for the result any more */
    dkpool_handle *out;
    DkrtStoreFile file;
} SpReq;

extern spa_pool sp_pool;
extern SpReq sp_reqs[SP_REQS];
extern volatile uint32_t sp_link_dead;   /* the receiver reported status 3: no new load can succeed until a boot */

/* xfer.c */
uint32_t sp_wire_size(void);
void *sp_copy_reply(void *destination, const void *source, uint32_t length);
void sp_xfer_tick(void);
void sp_xfer_cancel(SpReq *r);
#endif
