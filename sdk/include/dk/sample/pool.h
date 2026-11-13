/* Per-module stereo sample pool (16 entries; docs/design/sample-api-1.5.md). A pool entry holds one PCM16 sample, mono or stereo, in
   planar form; the runtime owns the memory and the module never sees a device
   address, a slot or a pool. */
#ifndef DK_SAMPLE_POOL_H
#define DK_SAMPLE_POOL_H
#include <dk/core/status.h>
#include <dk/gen/sample.h>
#include <stddef.h>
#include <stdint.h>

/* The states of an entry. */
#define DK_SAMPLE_UNUSED 0
#define DK_SAMPLE_QUEUED 1
#define DK_SAMPLE_LOADING 2
#define DK_SAMPLE_READY 3
/* The sample errors are the runtime status codes of <dk/core/status.h>. */
#define DK_SAMPLE_E_NOT_FOUND DK_E_NOT_FOUND
#define DK_SAMPLE_E_READ DK_E_READ
#define DK_SAMPLE_E_SHA DK_E_SHA
#define DK_SAMPLE_E_FORMAT DK_E_FORMAT
#define DK_SAMPLE_E_NOMEM DK_E_NOMEM
#define DK_SAMPLE_E_LOADED DK_E_LOADED
#define DK_SAMPLE_E_BUSY DK_E_BUSY
#define DK_SAMPLE_E_TOO_BIG DK_E_TOO_BIG
#define DK_SAMPLE_E_NO_DSP DK_E_NO_DSP
#define DK_SAMPLE_E_DSP_XFER DK_E_DSP_XFER
#define DK_SAMPLE_E_SEL_PATH DK_E_SEL_PATH
#define DK_SAMPLE_E_CONTRACT DK_E_CONTRACT

/* States are DK_SAMPLE_UNUSED/QUEUED/LOADING/READY (0..3); errors are negative.
   Reused errors: NOT_FOUND -1, READ -3, FORMAT -5, TOO_BIG -13, CONTRACT -48.
   In this family TOO_BIG means "does not fit the live pool". */
#define DK_SAMPLE_E_LOST                                                                           \
    DK_E_LOST /* the backing was lost (DT2 stock reset) and the reload failed */
#define DK_SAMPLE_E_POOL DK_E_POOL       /* the pool is full */
#define DK_SAMPLE_E_CHANGED DK_E_CHANGED /* the file differs from the selection snapshot */
/* DK_SAMPLE_E_SEL_PATH also means: sample_pool_path: buf too small. */

/* One record per pool entry in the module's DSP data, eight normal words, the runtime
   writes and the module reads. `ready` is published last. `token` is nonzero,
   changes on every load and never wraps or repeats. */
typedef struct dk_sample_pool_entry {
    uint32_t ready; /* 0 or 1 */
    uint32_t token;
    uint32_t frames;
    uint32_t rate;     /* Hz */
    uint32_t channels; /* 1 or 2 */
    uint32_t fmt;      /* DK_SAMPLE_POOL_FMT_PCM16 */
    uint32_t
        base_l; /* DSP normal-word address of plane L; DDR PCM uses the NW32 alias 0x10000000 + (phys - 0x80000000) / 4 (finding 99) */
    uint32_t base_r; /* plane R, same form; == base_l when channels == 1 */
} dk_sample_pool_entry;
/* The word offsets DK_SAMPLE_POOL_W_* and DK_SAMPLE_POOL_ENTRY_WORDS come from sdk.toml [sample] (<dk/gen/sample.h>); the assembly
   include dsp/contract/dk_sample_pool.inc has the same numbers. These checks tie the struct to them. */
_Static_assert(sizeof(dk_sample_pool_entry) == DK_SAMPLE_POOL_ENTRY_WORDS * 4u,
               "dk_sample_pool_entry layout");
_Static_assert(offsetof(dk_sample_pool_entry, ready) == DK_SAMPLE_POOL_W_READY * 4u,
               "dk_sample_pool_entry.ready");
_Static_assert(offsetof(dk_sample_pool_entry, token) == DK_SAMPLE_POOL_W_TOKEN * 4u,
               "dk_sample_pool_entry.token");
_Static_assert(offsetof(dk_sample_pool_entry, frames) == DK_SAMPLE_POOL_W_FRAMES * 4u,
               "dk_sample_pool_entry.frames");
_Static_assert(offsetof(dk_sample_pool_entry, rate) == DK_SAMPLE_POOL_W_RATE * 4u,
               "dk_sample_pool_entry.rate");
_Static_assert(offsetof(dk_sample_pool_entry, channels) == DK_SAMPLE_POOL_W_CHANNELS * 4u,
               "dk_sample_pool_entry.channels");
_Static_assert(offsetof(dk_sample_pool_entry, fmt) == DK_SAMPLE_POOL_W_FMT * 4u,
               "dk_sample_pool_entry.fmt");
_Static_assert(offsetof(dk_sample_pool_entry, base_l) == DK_SAMPLE_POOL_W_BASE_L * 4u,
               "dk_sample_pool_entry.base_l");
_Static_assert(offsetof(dk_sample_pool_entry, base_r) == DK_SAMPLE_POOL_W_BASE_R * 4u,
               "dk_sample_pool_entry.base_r");
#define DK_SAMPLE_POOL_FMT_PCM16 1u /* packed PCM16 (finding 87); float is not offered */

/* The per-entry control set the runtime and the bank macros of sdk/dsp/sample/bank.inc
   share: DK_SAMPLE_POOL_CTL_WORDS words per entry (sdk.toml [sample], <dk/gen/sample.h>). Offsets in words. */

/* Result of dk_sample_pool_info, 16 bytes, READY entries only. */
typedef struct dk_sample_info {
    uint32_t frames, rate, channels, token;
} dk_sample_info;
_Static_assert(sizeof(dk_sample_info) == 16, "dk_sample_info layout");

/* Peaks: immutable READY-token overview, built during loading. The caller
   supplies DK_SAMPLE_PEAK_BINS native-endian int16 min/max pairs (2048 bytes);
   success returns 1..DK_SAMPLE_PEAK_BINS bins,
   each covering ceil(info.frames/512) frames. Both channels contribute, extrema
   include zero. dk_sample_pool_peaks(p,index,info.token,out,sizeof out) rejects a
   stale token or non-READY entry without touching out. Peaks may be unavailable
   even while the sample stays READY/playable. No PCM scan or IO occurs.
   UI task only. */
typedef struct dk_sample_peak {
    int16_t min, max;
} dk_sample_peak;

/* Peak window: dk_sample_pool_peaks_window(p, index, token, first, frames, columns, out, capacity) gives min/max pairs (dk_sample_peak) of
   `columns` equal parts of the frames [first, first + frames) of a READY entry. The runtime reads the PCM in the background. It returns
   `columns` when out is filled, 0 while the window is computed (call again from the draw callback with the same arguments), or a
   negative error. A call with other arguments replaces a pending window; one window is kept per module. columns is 1..DK_SAMPLE_WINDOW_MAX
   and capacity is at least columns * 4 bytes. UI task only. */

/* The imports (<dk/platform/samples.h>, <dk/gen/imports.h>): `dk_sample_pool_open(ctx, &pool)` in `init` gives the handle `p` of the module's one pool of
   DK_SAMPLE_POOL_ENTRIES (16) entries; the entry index is the record index the DSP reads (a machine picks it with a parameter, for example SHOT's SMP):
     int32_t dk_sample_pool_add(p, const char *path);            the pool index 0..15, or a negative error
     int32_t dk_sample_pool_set(p, index, const char *path);     0 or error: load into that index, replacing what was there
     int32_t dk_sample_pool_clear(p, index);                     0 or error; index >= 16: E_CONTRACT
     int32_t dk_sample_pool_status(p, index);                    sticky errors until the entry is added again or cleared
     int32_t dk_sample_pool_info(p, index, void *out);           out is a dk_sample_info
     int32_t dk_sample_pool_path(p, index, char *buf, uint32_t cap);   length, or error
     int32_t dk_sample_pool_peaks(p, index, token, void *out, uint32_t capacity);   bins, or error
   set loads the file into the given index (a filled entry is replaced in place, the old file is released; a file another entry holds is
   shared); index >= 16: E_CONTRACT. add returns the first UNUSED index. A file that is already in the pool (the same path) returns its index and loads nothing; if that
   entry has a sticky error, the add selects it again. Indexes are stable: clear leaves a hole and never moves another entry, so a value
   saved in a sound or a p-lock keeps its meaning. A full pool returns DK_SAMPLE_E_POOL. An entry that is not READY (UNUSED, QUEUED,
   LOADING, an error) is silent in the DSP: note_on is denied and the voice ends. The pool is kept per project; a project change
   reloads the list. Calls are legal from init and service/UI callbacks. A runtime image without a sample backing returns
   DK_SAMPLE_E_CONTRACT from dk_sample_pool_add.

   Paths. dk_sample_pool_add takes the root-relative path of a DK_FILES_SAMPLES listing
   row, as <dk/platform/files.h> gives it ("KICKS/K1", no leading slash); the runtime adds the device's
   root. A DT2 path that already starts with '/' (an older form) is accepted as it is.
   dk_sample_pool_path returns the path of an entry in the root-relative form, in
   every state but UNUSED (E_NOT_FOUND): it is what a UI shows after a project change
   reloaded the list. It copies the NUL-terminated path into buf and returns its length
   without the NUL; if length + 1 > cap it returns DK_SAMPLE_E_SEL_PATH and leaves buf alone.
   */
#endif
