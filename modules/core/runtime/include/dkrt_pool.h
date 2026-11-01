/* The sample backing plug-in point, version 1. NOT an SDK contract: content modules never see it.
   The runtime (device-neutral) calls the optional symbols dkpool_* that a device adapter
   exports (modules/core/adapters/sample-pool); the adapter calls dkrt_pool_done and
   dkrt_pool_lost. Without the adapter the symbols resolve to 0 and sample_select returns
   DK_SAMPLE_E_CONTRACT. docs/design/sample-api-1.5.md, section 2.2.
   All calls run in the runtime's task context. `id` and `path` belong to the runtime;
   the adapter does not keep them past the call. Additive growth only: new symbols are
   optional, existing signatures never change. */
#ifndef DKRT_POOL_H
#define DKRT_POOL_H
#include <stdint.h>

#define DKPOOL_VERSION 1u
/* dkpool_version() returns DKPOOL_VERSION; the runtime refuses a different value. */
uint32_t dkpool_version(void);

/* Errors are the runtime's load codes (sdk/sdk.toml [load.status]). */
#define DKPOOL_E_READ (-3)
#define DKPOOL_E_FORMAT (-5)
#define DKPOOL_E_LOST (-15)
#define DKPOOL_E_POOL (-16)
#define DKPOOL_DONE 0   /* dkpool_load finished, `out` is filled */
#define DKPOOL_QUEUED 1 /* dkrt_pool_done(req, result) follows later */

/* File identity snapshot taken by the runtime at select. Two selections with equal
   identity share one backing handle. */
#define DKPOOL_ID_HASH_VALID 1u /* hash_kind/hash from the listing row are valid */
typedef struct dk_file_id {
    uint32_t root;      /* DK_FILES_SAMPLES or DK_FILES_STORE */
    uint32_t size;      /* source physical bytes */
    uint32_t mtime;     /* 0 when unknown; used when no hash is valid */
    uint32_t flags;     /* DKPOOL_ID_* */
    uint32_t hash_kind; /* DK_FILE_HASH_* */
    uint8_t hash[32];
} dk_file_id;

/* What the runtime stores per dedup entry. */
typedef struct dkpool_handle {
    uint32_t id;      /* adapter-private (DT2: slot; DN2: block index) */
    uint32_t token;   /* nonzero, unique per load, never reused */
    uint32_t base_l;  /* normal-word address of plane L (as in dk_strack) */
    uint32_t base_r;  /* plane R; == base_l for mono */
    uint32_t frames;
    uint32_t rate;    /* Hz */
    uint32_t channels;/* 1 or 2 */
} dkpool_handle;

/* Alloc, read, convert, make DSP-visible. `req` is chosen by the runtime and returned by
   dkrt_pool_done; `out` stays runtime-owned and valid until the load is terminal and the
   adapter fills it before it reports DONE. Returns DKPOOL_DONE, DKPOOL_QUEUED or a negative
   error (E_POOL pool full, E_READ, E_FORMAT). Reusing a slot of the same file is allowed. */
int32_t dkpool_load(uint32_t req, const dk_file_id *id, const uint8_t *path, dkpool_handle *out);
/* PCM of other handles never moves. */
void dkpool_release(const dkpool_handle *h);
/* A few loads. 0 valid, DKPOOL_E_LOST when the backing was lost externally. */
int32_t dkpool_check(const dkpool_handle *h);
uint32_t dkpool_free_frames(void);

/* Adapter to runtime. result: 0, DKPOOL_E_POOL, DKPOOL_E_READ or DKPOOL_E_FORMAT. */
void dkrt_pool_done(uint32_t req, int32_t result);
/* "Backing lost": all handles are invalid (reset). */
void dkrt_pool_lost(void);
/* Project adapter to runtime: the current project slot changed. The runtime never reads the byte. */
void dkrt_project_changed(uint32_t slot);
#endif
