/* <dk/sample/decl.h>: the declaration of the sample table of a PATCHED-IN module (`dynamic = false`; patchkit adds this directory for those builds only).
   A run-time module gets its pool from the runtime (`dk_sample_pool_open`, <dk/sample/pool.h>). Hand-written. */
#ifndef DK_SAMPLE_DECL_H
#define DK_SAMPLE_DECL_H
#include <dk/page/decl.h>
#include <dk/sample/pool.h>

/* Declares the table: RECORDS names 16 zeroed dk_sample_pool_entry in the module's DSP data
   (DK_SAMPLE_POOL_ENTRIES * 8 words); CONTROL names 16 control sets of 4 zeroed words.
   The runtime fills the records; the module never writes them. */
#define DK_SAMPLE_TRACKS(LOCAL_ID, RECORDS, CONTROL)                                               \
    static const dk_desc_sample_tracks DK_CAT(dk_sample_tracks_, __LINE__)                         \
        DK_DESC = {.kind = DK_DESC_SAMPLE_TRACKS,                                                  \
                   .size = sizeof(dk_desc_sample_tracks),                                          \
                   .local_id = (LOCAL_ID),                                                         \
                   .records = #RECORDS,                                                            \
                   .control = #CONTROL}

#endif
