# Draft: stereo sample binding, API 1.5

Status: draft for Em's review (revised 2026-11-01: device code moved out of core/runtime into core/adapters/sample-pool). No code. Sources: sdk/include/dk/sample.h (1.3 bank), files.h (1.4), gen/machine.h (contract 1.1, `track`), sdk/dsp/sample_bank.inc, sample_pcm16*.inc, docs/design/sample-binding.md, findings 84, 87, 94, 105, 81 (and 96, 99 for the stereo kernels).

## 1. Module-facing API (C header sketch)

Decision: add a new header `dk/sample_track.h` and new Api entries (minor 5). Keep `dk/sample.h` as is. Nothing in 1.4 changes.

```c
/* dk/sample_track.h -- per-track stereo samples. API contract 1.5. */
#define DK_STRACKS 16u
/* states: reuse DK_SAMPLE_UNUSED/QUEUED/LOADING/READY (0..3); errors < 0 */
#define DK_SAMPLE_E_CHANGED (-14)   /* file identity differs from the selection snapshot */
#define DK_SAMPLE_E_LOST    (-15)   /* backing wiped (DT2 stock reset), reload failed */
#define DK_SAMPLE_E_POOL    (-16)   /* pool full */
/* reuse: NOT_FOUND -1, READ -3, FORMAT -5, TOO_BIG -13, CONTRACT -48.
   TOO_BIG now means "does not fit the live pool", not "> 8192 frames". */

/* DSP record, one per track, 8 words, runtime writes, module reads. */
typedef struct dk_strack {        /* normal-word DM view */
    uint32_t ready;               /* 0/1, published last */
    uint32_t token;               /* nonzero generation; never reused, no wrap */
    uint32_t frames;
    uint32_t rate;                /* Hz */
    uint32_t channels;            /* 1 or 2 */
    uint32_t fmt;                 /* 1 = packed PCM16 (finding 87); float is not offered */
    uint32_t base_l;              /* normal-word address of plane L */
    uint32_t base_r;              /* plane R; == base_l when channels == 1 */
} dk_strack;
#define DK_SAMPLE_TRACKS(LOCAL_ID, TABLE) /* new desc kind DK_DESC_SAMPLE_TRACKS:
    16 zeroed dk_strack records in module BSS plus the epoch/count/close words
    that sample_bank.inc already uses, one set per track */

/* Api table additions (ColdFire side, resolved/used like sample_bank in init) */
i32 sample_tracks(u32 local_id);                       /* init: handle > 0, or error */
i32 sample_select(u32 h, u32 track, const u8 *path);   /* queue; copies path; stops that track's old notes; 0 or error */
i32 sample_unbind(u32 h, u32 track);                   /* close generation, release refs; 0 or error */
i32 sample_track_status(u32 h, u32 track);             /* UNUSED/QUEUED/LOADING/READY or <0 (sticky until next select/unbind) */
i32 sample_track_info(u32 h, u32 track, dk_sample_info *out); /* frames, rate, channels, token; READY only */
```

Calls are legal from init and service/UI callbacks. `track >= 16` returns `E_CONTRACT`. `select` while QUEUED/LOADING supersedes: the old request is dropped, the status goes QUEUED again. `E_BUSY` is not used; supersession replaces it.

What the DSP reads at note_on: `in->track` (contract 1.1) picks `dk_strack[track]`. Using the existing bank macros in one masked scope: CHECK_ADMISSION, then LATCH_EPOCH, then copy `base_l`, `base_r`, `frames`, `channels`, `rate`, `token` into the voice state (7 words, add to `state_bytes`). Denied: `note_on` returns 0 and render returns `DK_DONE` at once (silent).

Stable view during a note (no `note_off`, so no lease held across blocks, per design doc): the voice keeps its snapshot and the token. Each render does CHECK_EPOCH + ACQUIRE in one masked scope, reads, then RELEASE. A stale token (select, unbind, stock reset) returns `DK_DONE` without touching PCM. The runtime guarantees PCM does not move while the token is current, on both devices.

Mono 1.4 users: unchanged. `sample_bank`/`sample_load`/`DK_SAMPLE_BANK` keep the 4-word descriptor, one bank, 8,192 frames, float. A module may not use both families (bank + tracks); the runtime refuses the second with `E_CONTRACT`. Mono files loaded through 1.5 give channels=1; the stereo kernel path can read `base_r = base_l`, so one kernel serves both.

## 2. Split: device-neutral runtime, device-specific sample-pool adapter

Rule (ARCHITECTURE.md): anything that depends on device or firmware version sits in adapters, profiles and anchors. `core/runtime` has no `cf.*` anchor and no address for sample memory. It reaches the device through one plug-in point, the **sample backing**, as optional symbols (`dkpool_*`, declared in a new `dkrt_pool.h`). `core/adapters/sample-pool` implements them. Without the adapter the symbols resolve to 0 and `sample_select` returns `E_CONTRACT`.

### 2.1 core/runtime (device-neutral)

- Entries of `dk/sample_track.h` (API 1.5) and the 16-track table.
- Preflight at `select`, before any backing change: open file, check header (format, channels 1/2, rate, PCM16), size arithmetic (frames, plane words P = 2*C*N for stereo planar), identity snapshot (size + hash from the 1.4 listing row, plus open-time stat). Fail with FORMAT / NOT_FOUND / TOO_BIG without touching the old binding.
- Dedup: key = file identity (path root + hash/size). Refcount = tracks bound. Same file on two tracks shares one backing handle. Last unref calls `release`.
- Load state machine per track: UNUSED / QUEUED / LOADING / READY / ERROR (section 3). Completion matching on (client, track, request token); counter exhaustion fails, never wraps.
- Stop-old-notes: on `select`/`unbind`, CLOSE the track generation, wait for the DSP drain ack (never spin in an interrupt), only then call the backing. Other tracks are not touched.
- Publish: write `dk_strack`, `ready` last, after load, identity check and ACK.
- Project change and per-project selection store (section 3). Project identity comes from the project adapter (2.4).

### 2.2 Backing plug-in point (`dkrt_pool.h`, version 1)

All calls run in the runtime's task context. `identity` and `path` are runtime-owned; the adapter does not keep `path`.

```c
typedef struct dkpool_handle {          /* what the runtime stores per dedup entry */
    uint32_t id;                        /* adapter-private (DT2: slot; DN2: block index) */
    uint32_t token;                     /* nonzero, unique per load; never reused */
    uint32_t base_l, base_r;            /* normal-word addresses of the planes (as in dk_strack) */
    uint32_t frames;
} dkpool_handle;
i32  dkpool_load(const dk_file_id *id, const u8 *path, dkpool_handle *out);
        /* alloc + read + convert + DSP-visible; may be async: returns QUEUED, then
           dkrt_pool_done(req, result) from the adapter; result: 0, E_POOL, E_READ, E_FORMAT */
void dkpool_release(const dkpool_handle *h);   /* PCM of other handles never moves */
i32  dkpool_check(const dkpool_handle *h);     /* 0 valid; E_LOST if the backing was lost externally */
u32  dkpool_free_frames(void);                 /* free-space query, for TOO_BIG and the UI */
/* adapter -> runtime event */
void dkrt_pool_lost(void);                     /* "backing lost": all handles are invalid (reset) */
```

Guarantees the adapter gives: PCM of a valid handle does not move; `token` changes on every load; `dkpool_check` is cheap (a few loads) so the runtime can call it from its periodic tick and before every publish. On `dkrt_pool_lost` or a failed check, the runtime CLOSEs the affected tracks, sets LOADING and reloads from the remembered path and identity; a failed reload gives ERROR `E_LOST` or `E_CHANGED`.

### 2.3 core/adapters/sample-pool (per device)

Anchors live in a new `devices/fw/samples.toml` (rule kinds as in `fw/storage`: `cf_func` hashes, `users = ["core/sample-pool"]`) and resolve per firmware through `devices/ports/*.toml`. The adapter binds them through the generated `dk_fw_samples.h` / crate `dkfw`, like `storage-fault-proof` binds `cf.storage.site.*` through `[object.externs]`. Values below are DT2 1.17; the port files hold the real per-image addresses.

**DT2 1.17: borrow the stock sample pool** (emulator-confirmed, `tests/regression/runtime/stock-pool-borrow.test.ts`).
- Loader `0x4015481c(keyptr, slot)`. It runs as a queued job on the SampleLoaderBgWorker (getter `0x4019e65c`, queue `0x400f0c1c`), as ReloadAllSamples does; `dkpool_load` queues and completes asynchronously.
- 16-byte key built with the stock path open / metadata / close functions `0x40158902` / `0x4015aec2` / `0x401588c0`. Slot key array: `0x405b1368 + slot*0x10`. DSP slot record: `0x257810 + slot*0x14`.
- PCM address: word0 of the DSP record minus 0x40 equals the pool block offset; PCM byte address = `0x8422b9c8 + offset`.
- Slots from 0x3ff downward; the floor comes from the device profile. Pool full returns -1, mapped to `E_POOL`.
- Never call the stock assign wrapper `0x4004b502`: it writes the Elektron project.
- Reset: stock `ReloadAllSamples(..., 1)` wipes our slots about 1.5 s after the call. Detect it by key word 0 becoming `0xffffffff` or the DSP record length (record + 8) becoming 0. Do not use the slot map (`0x405b5368`): it reads 0 after a reset. `dkpool_check` does this test; a short tick in the adapter raises `dkrt_pool_lost` when it sees it.
- Release compaction never moves PCM. A stock slot of the same file is reused (dedup is free).

**DN2 1.12: own allocator** over a reserved high-DDR range from the device profile. The canary soak saw no stock writes in [0x80531000, 0xa0000000), about 507 MiB, in 14 runs; the final size is set after an extended soak. First-fit with the same dedup; the gap [0x8052d790, 0x8052fbe0) (9 KiB) is stock-owned and stays out. No external reset exists; `dkrt_pool_lost` is never raised. The reserved range is a profile field, not a constant in the adapter.

**Stereo.** DT2 writes R into the odd voice buffer of the stock voice pair (stock pans the two buffers independently, finding 96). DN2 uses the shadow right-channel stages made production (the `core/dsp-stereo-proof` route). Both sit on the dsp-voice adapter side, not in the runtime and not in the pool plug-in; the handle only carries `base_l` and `base_r`.

### 2.4 Project identity

Key = (device, slot, project name). The current project slot byte is an anchor: DT2 `0x4099d5c5` with setter `0x400c11ca`; DN2 `0x405ce899` with setter `0x400bb7fa`. Put them in `devices/fw/project.toml` and let a project adapter hook the setter and call the runtime (`dkrt_project_changed(slot)`); the runtime never reads the byte. The project-name accessor is still open.

### 2.5 Content modules

A SPARK machine or picker sees only `dk/sample_track.h`: no slot, no address, no device.

## 3. Lifecycle (per track)

```
UNUSED --select--> QUEUED --preflight ok--> [CLOSE old gen, drain] --> LOADING
LOADING --payload + identity check + ACK--> READY (token n+1 published)
QUEUED|LOADING --select--> QUEUED            (supersede; old transfer finishes, result dropped)
QUEUED|LOADING --error--> ERROR(code)        (silent, sticky)
READY --select--> QUEUED                     (old notes stop at CLOSE)
READY|ERROR --unbind--> UNUSED               (close, unref)
READY --backing lost (dkrt_pool_lost / dkpool_check)--> LOADING (reload) --> READY | ERROR(LOST/CHANGED)
ERROR --select--> QUEUED
```

Project change: digikit's store holds per-track selections keyed by project (identity from 2.4). On change: unbind all 16 (close, drain, unref), look up the new project's selections, `select` each. Missing file = ERROR NOT_FOUND on that track only; no retry. Pool entries of files used by both projects survive through dedup when select runs before unbind frees (do selects first, then drop old refs).
Backing loss: see 2.2 and 2.3. A stock project change on DT2 usually also resets the pool, so the project-change path and the reset path meet: reset detection then finds our selections already re-driven by the store and does not double-load (load request is idempotent per file identity).
Module unload: unbind all, drain, free; teardown waits for terminal completion of any transfer.

## 4. Versioning

- Api minor 4 -> 5 (`api = [1, 5]`): five entries appended (`sample_tracks`, `sample_select`, `sample_unbind`, `sample_track_status`, `sample_track_info`). Additive: a 1.4 module runs on a 1.5 runtime unchanged. A 1.5 module on a 1.4 runtime is refused by the existing contract check.
- Machine contract stays 1.1 (`track` already there). No change to `dk_voice_in`.
- New descriptor kind `DK_DESC_SAMPLE_TRACKS` (additive; a runtime that does not know it refuses the module by size/kind check).
- New error codes -14..-16 appended; existing values fixed. `E_TOO_BIG` meaning widens for the track family only.
- Breaking, avoided: record layout is versioned by `size` in the descriptor; new fields append at the end (max 8 words now; extend by raising `size`, never by reordering). Float fmt and >2 channels are deferred, not reserved bits.
- The plug-in point `dkrt_pool.h` has its own version (`DKPOOL_VERSION 1`); the adapter exports it, the runtime checks it at link time and refuses a mismatch. It is not an SDK contract: content modules never see it.
- sdk.toml and the generated headers/Rust bindings change in one `patchkit gen` run; `DK_MAX` voice state grows by 7 words for sampler modules only.

## 5. Open questions for Em

Resolved by the split and the emulator results (dropped): where the DT2 slot map is the truth (it is not; use key and DSP record), whether the DT2 borrow works, how DN2 pool size is stored (profile field).

1. One header and entries as above, or extend `dk/sample.h`? Recommend the new header; the old family stays frozen and mono-only.
2. Voice view: snapshot + per-render epoch check, versus a lease for the whole note? Recommend snapshot + check (no `note_off`; leases leak on stolen voices).
3. PCM16 as the only stored format in 1.5? Recommend yes; float later as additive `fmt`.
4. Expose free bytes to modules? Recommend `sample_track_info` failure detail plus read-only `pool_free_frames` for the UI, fed by `dkpool_free_frames`; no reservation API.
5. DT2 slot floor: profile-driven. Recommend yes; freeze the default after one more stock-project-load test.
6. Missing/changed file after project load: silent per-track ERROR, no auto-retry. Recommend yes for the first release.
7. Identity strength: listing hash when present, else size + mtime snapshot; mismatch is `E_CHANGED`. Recommend that.
8. Bank (1.3) and tracks (1.5) in one module? Recommend no.
9. New: one `core/sample-pool` adapter with per-device objects (`src/cf/dt2/`, `src/cf/dn2/`, as ARCHITECTURE.md step 2 says), or two adapters (`sample-pool-dt2`, `sample-pool-dn2`)? Recommend one adapter, two objects: one manifest, one header, one test set; the DN2 allocator and the DT2 borrow share only the interface, so the split is by object, not by module.
10. New: plug-in point versioning. Recommend an integer `DKPOOL_VERSION` exported by the adapter and checked by the runtime, additive growth only, same rules as the Api table; no separate manifest field.
11. New: is `dkpool_load` async (queued job on DT2) or does the runtime own the wait? Recommend async with a completion call, so the runtime state machine is the only waiter.
12. New: who raises "backing lost" on DT2, a tick in the adapter or the runtime polling `dkpool_check`? Recommend both: the adapter tick for latency (reset is about 1.5 s late), runtime check before each publish as the safety net.
13. New: DN2 reserved range size. Recommend 256 MiB in the profile now, raise after the extended canary soak; the soak result goes in `docs/findings/`.
14. New: project-name accessor (2.4). Recommend static search for the stock name getter first; fall back to hashing the project's first serialized block only if none exists.
