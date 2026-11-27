/* <dk/host/voice_host.h>: VoiceHost v1, the table of a platform host that plays a digikit machine voice on a track. Hand-written.

   A platform host (core/peer-audio now, core/dsp-voice later) publishes one `[[info]]` table named `voice_host` in the image info table. The machines service finds it with
   `CoreApi::info(name_hash("voice_host"))` and calls it. A device has one host. The Rust mirror is `dkrt_sdk::voice_host` (runtime/core/sdk), and a host test checks that both layouts agree.

   Rules:
   - Every call runs in the UI task and does not block. A call returns DK_OK, DK_VH_LOADING (the work goes on or the host is busy: call again, see the continuation rule) or a negative DK_E_* code.
   - `type_out` is in and out. On entry, a non-zero value is the machine type the service requests (on SHARC it is the firmware machine type that stored projects use, so the service chooses it). The host uses exactly that type or refuses with an error; it never returns a different type. Zero on entry means the host chooses and writes the type it chose.
   - Continuation: when `install` answers DK_VH_LOADING with a non-zero `*type_out`, the request is accepted and `*type_out` holds the type it occupies (the requested one, or the one the host chose now). The caller continues by calling `install` again with that exact type in `*type_out` and `payload` NULL. A call with a NULL payload and the type of the install in progress is a continuation; so a silent-voice request (NULL payload, 0 bytes) for the type that is in flight is taken as a continuation too. Any other call while an install is in progress, also one identical to the first, answers DK_VH_LOADING with `*type_out` set to 0: it is not accepted, and the caller calls again later with its full request. The host has one staging buffer, so one install is in flight; it ends with DK_OK, an error, or `retire` of its type. An install that its caller abandons holds the buffer until that `retire`, which frees the buffer for the next install.
   - An `install` with no payload (`payload` NULL and `payload_bytes` 0), made when no install of that type is in flight, installs the silent voice: it reserves a type that plays silence, or, for a type that is already installed, parks it in place (its tracks play silence and the type number stays reserved). A later `install` with a payload for a parked type takes it over. `retire` frees a silent or parked type like any other.
   - No retained pointers. The host copies the payload during the first `install` call for a request (it places it in host-owned memory or a host-owned staging buffer); the caller's staging memory is reused afterwards. Continuations pass `payload` NULL and must be accepted. Once any call returns, the host holds no pointer into `req`, the payload, or any service or module memory. The host keeps no callbacks into a service. So a service reload can retire its types and install them again from the new copy.
   - The payload format belongs to the host kind and carries its own header and version. The service never parses it; patchkit writes it from the module's `requires`.
   - Control values do not pass through this API. Each host reads the firmware's track knob words itself.
   - `install` refuses with DK_E_NOMEM when `cost * desc->voices` does not fit `budget`, or when `types_max`, `payload_bytes_max` or `state_bytes_max` is exceeded.
   - `bind` is called for every host when a track's machine changes. Type 0 is the stock machine.
   - `retire` may return DK_VH_LOADING ("call again"), like `install`. While a type retires, every track bound to it plays the stock fallback (silent for digikit). The host releases the type's placed code and voice state only when `retire` returns DK_OK. A consumer keeps calling `retire` until it returns DK_OK or an error.
   - `stats` feeds PROFILE_DSP rows and HEALTH.
   - New fields go only at the end of dk_voice_host_v1; `size` says how many bytes the host wrote. A consumer needs the same major and at least the minor it knows.

   Layout, with P the size of a pointer (4 on the ColdFire):
   dk_voice_install: payload 0, payload_bytes P, desc align(P+4,P), range_min desc+P, range_max range_min+32, cost range_max+32, size align(cost+4,P).
   dk_voice_stats: six u32, 24 bytes.
   dk_voice_host_v1: magic 0, major 4, minor 6, size 8, tracks 12, types_max 14, payload_bytes_max 16, state_bytes_max 20, budget 24, flags 28, ctx 32, install 32+P, bind 32+2P, retire 32+3P, stats 32+4P, size 32+5P. */
#ifndef DK_HOST_VOICE_HOST_H
#define DK_HOST_VOICE_HOST_H

#include <stdint.h>
#include <dk/core/status.h>
#include <dk/gen/machine.h>

#define DK_VOICE_HOST_NAME                                                                         \
    "voice_host" /* the name of the info table; its hash is the FNV-1a of these bytes */
#define DK_VH_MAGIC 0x444b5648u /* 'DKVH' */
#define DK_VH_MAJOR 1
#define DK_VH_MINOR 0
#define DK_VH_LOADING                                                                              \
    1 /* the status of a call that is not finished (the value of DKRT_LOADING in <dkrt.h>) */
#define DK_VH_RANGES 8 /* range entries of an install: DK_MAX_PARAMS */

/* dk_voice_host_v1.flags */
#define DK_VH_STEREO 0x1u /* the host can play a stereo voice */

typedef struct dk_voice_install {
    const void *
        payload; /* the voice object as patchkit packed it for this host; opaque to the service. NULL with payload_bytes 0: the silent voice. The host copies it during the first install call; a continuation passes NULL */
    uint32_t payload_bytes;
    const dk_machine_desc
        *desc; /* voices, state_bytes, state_clear_bytes, n_params, stack_bytes, max_call_depth */
    int32_t range_min[DK_VH_RANGES]; /* the machine's [[param]] ranges, for the host's clamp */
    int32_t range_max[DK_VH_RANGES];
    uint32_t cost; /* dsp_cost_voice from the module's [uses], per voice and block */
} dk_voice_install;

typedef struct dk_voice_stats {
    uint32_t calls, sum, max_call, max_block, overruns, faults;
} dk_voice_stats;

typedef struct dk_voice_host_v1 {
    uint32_t magic;        /* DK_VH_MAGIC */
    uint16_t major, minor; /* DK_VH_MAJOR, DK_VH_MINOR */
    uint32_t size;         /* bytes of this table */
    uint16_t tracks;       /* tracks the host can play a digikit voice on */
    uint16_t types_max;    /* voice types installed at once */
    uint32_t payload_bytes_max;
    uint32_t state_bytes_max; /* per voice */
    uint32_t budget;          /* cost units per block the host admits (device data) */
    uint32_t flags;           /* DK_VH_STEREO, ... */
    void *ctx;                /* the first argument of every call */
    /* type_out is in and out: non-zero on entry is the type requested (used exactly or refused), zero lets the host choose and write it. On DK_VH_LOADING a non-zero value is the type the request occupies: continue with it and a NULL payload; 0 means not accepted yet. */
    int32_t (*install)(void *ctx, const dk_voice_install *req, uint32_t *type_out);
    int32_t (*bind)(void *ctx, uint32_t track, uint32_t type); /* type 0: the stock machine */
    int32_t (*retire)(void *ctx, uint32_t type);
    int32_t (*stats)(void *ctx, uint32_t type, dk_voice_stats *out);
} dk_voice_host_v1;

#endif
