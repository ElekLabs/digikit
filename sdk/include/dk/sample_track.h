/* Per-track stereo samples. API contract 1.5 (docs/design/sample-api-1.5.md).
   Additive to <dk/sample.h> (1.3 bank, frozen, mono, 8192 frames, float): a module
   uses the bank family or the track family, never both (the second is refused with
   DK_SAMPLE_E_CONTRACT). A track holds one PCM16 sample, mono or stereo, in
   planar form; the runtime owns the memory and the module never sees a device
   address, a slot or a pool. */
#ifndef DK_SAMPLE_TRACK_H
#define DK_SAMPLE_TRACK_H
#include <dk/sample.h>
#include <dk/gen/desc.h>

#define DK_STRACKS 16u

/* States are DK_SAMPLE_UNUSED/QUEUED/LOADING/READY (0..3); errors are negative.
   Reused errors: NOT_FOUND -1, READ -3, FORMAT -5, TOO_BIG -13, CONTRACT -48.
   In this family TOO_BIG means "does not fit the live pool". */
#define DK_SAMPLE_E_LOST (-15)    /* the backing was lost (DT2 stock reset) and the reload failed */
#define DK_SAMPLE_E_POOL (-16)    /* the pool is full */
#define DK_SAMPLE_E_CHANGED (-17) /* the file differs from the selection snapshot */
/* The design draft numbers E_CHANGED -14; -14 is E_TABLE_FULL, so it is -17. */

/* One record per track in the module's DSP data, eight normal words, the runtime
   writes and the module reads. `ready` is published last. `token` is nonzero,
   changes on every load and never wraps or repeats. */
typedef struct dk_strack {
    uint32_t ready;    /* 0 or 1 */
    uint32_t token;
    uint32_t frames;
    uint32_t rate;     /* Hz */
    uint32_t channels; /* 1 or 2 */
    uint32_t fmt;      /* DK_STRACK_FMT_PCM16 */
    uint32_t base_l;   /* normal-word address of plane L */
    uint32_t base_r;   /* plane R; == base_l when channels == 1 */
} dk_strack;
_Static_assert(sizeof(dk_strack) == 32, "dk_strack layout");
#define DK_STRACK_WORDS 8u
#define DK_STRACK_FMT_PCM16 1u /* packed PCM16 (finding 87); float is not offered */

/* The per-track control set the runtime and the bank macros of sdk/dsp/sample_bank.inc
   share: four words per track. Offsets in words. */
#define DK_STRACK_CTL_CLOSED 0u
#define DK_STRACK_CTL_COUNT 1u
#define DK_STRACK_CTL_FAULT 2u
#define DK_STRACK_CTL_WORDS 4u

/* Result of sample_track_info, 16 bytes, READY tracks only. */
typedef struct dk_sample_info {
    uint32_t frames, rate, channels, token;
} dk_sample_info;
_Static_assert(sizeof(dk_sample_info) == 16, "dk_sample_info layout");

/* Declares the table: RECORDS names 16 zeroed dk_strack in the module's DSP data
   (DK_STRACKS * 8 words); CONTROL names 16 control sets of 4 zeroed words.
   The runtime fills the records; the module never writes them. */
#define DK_SAMPLE_TRACKS(LOCAL_ID, RECORDS, CONTROL)                                \
    static const dk_desc_sample_tracks DK_CAT(dk_sample_tracks_, __LINE__) DK_DESC = { \
        .kind = DK_DESC_SAMPLE_TRACKS, .size = sizeof(dk_desc_sample_tracks),      \
        .local_id = (LOCAL_ID), .records = #RECORDS, .control = #CONTROL}

/* Api entries (resolved in init like sample_bank; see <dk/gen/api.h>):
     int32_t sample_tracks(uint32_t local_id);                 handle > 0, or error
     int32_t sample_select(h, track, const uint8_t *path);     0 or error; track >= 16: E_CONTRACT
     int32_t sample_unbind(h, track);
     int32_t sample_track_status(h, track);                    sticky errors until select/unbind
     int32_t sample_track_info(h, track, uint8_t *out);        out is a dk_sample_info
   select while QUEUED or LOADING supersedes (status QUEUED again). Calls are legal
   from init and service/UI callbacks. A runtime image without a sample backing
   returns DK_SAMPLE_E_CONTRACT from sample_select. */
#endif
