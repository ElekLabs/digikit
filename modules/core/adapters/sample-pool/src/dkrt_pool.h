/* dkrt_pool.h, version 1: the sample backing plug-in point (docs/design/sample-api-1.5.md 2.2).
 *
 * PROVISIONAL COPY. core/runtime owns this header (agent 1a); this copy follows the accepted design so that core/sample-pool builds
 * alone. The coordinator reconciles the two at merge. Differences to settle: the completion call names its request by the `out`
 * pointer of dkpool_load; dkpool_tick and dkpool_version are additions of this adapter; dk_file_id is a placeholder (the DT2 object
 * does not read it).
 *
 * The adapter exports dkpool_*; the runtime exports dkrt_pool_done and dkrt_pool_lost. The runtime calls every dkpool_* function in
 * its UI-loop task. The adapter never keeps `path` after dkpool_load returns, and never calls into the runtime from another task. */
#ifndef DKRT_POOL_H
#define DKRT_POOL_H
#include <stdint.h>

#define DKPOOL_VERSION 1u

/* Result codes, the numbers of docs/design/sample-api-1.5.md section 1 and of sdk/sdk.toml [load.status]. */
#define DKPOOL_OK 0
#define DKPOOL_QUEUED 1          /* dkpool_load took the request; the result comes with dkrt_pool_done */
#define DKPOOL_E_NOT_FOUND (-1)
#define DKPOOL_E_READ (-3)
#define DKPOOL_E_FORMAT (-5)
#define DKPOOL_E_CONTRACT (-48)  /* caller broke the contract: too many requests, token counter exhausted */
#define DKPOOL_E_LOST (-15)
#define DKPOOL_E_POOL (-16)

/* File identity the runtime snapshots at preflight (the 1.4 listing row). */
typedef struct dk_file_id {
    uint32_t size_bytes;
    uint32_t hash_kind;
    uint8_t hash[32];
} dk_file_id;

typedef struct dkpool_handle {
    uint32_t id;               /* adapter-private (DT2: stock slot; DN2: block index) */
    uint32_t token;            /* nonzero, unique per load, never reused */
    uint32_t base_l, base_r;   /* normal-word (NW32 DDR) addresses of the planes; base_r == base_l for mono */
    uint32_t frames;
} dkpool_handle;

uint32_t dkpool_version(void);
/* Queue a load: copies `path`, returns DKPOOL_QUEUED, or a negative code at once (E_POOL: no free slot). Later the adapter calls
 * dkrt_pool_done(out, result) from dkpool_tick, with `out` filled on result 0. `out` and `id` stay valid until then. */
int32_t dkpool_load(const dk_file_id *id, const uint8_t *path, dkpool_handle *out);
void dkpool_release(const dkpool_handle *h);   /* the PCM of other handles never moves */
int32_t dkpool_check(const dkpool_handle *h);  /* 0 valid; DKPOOL_E_LOST if the backing was lost */
uint32_t dkpool_free_frames(void);             /* free space in mono PCM16 frames (a stereo frame costs two) */
void dkpool_tick(void);                        /* once per UI-loop pass: completions, reset detection */

void dkrt_pool_done(dkpool_handle *out, int32_t result);
void dkrt_pool_lost(void);                     /* every handle is invalid: a stock reset */
#endif
