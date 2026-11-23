//! The C entry points of the samples service file: the nine imports a module calls (`dk_sample_pool_*`, the handle first) and the two slots the sample-pool
//! adapter calls. The handle a module holds is its client number, as a pointer (never null: a client is 1 or more); each call resolves it inside the service.
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::{E_CONTEXT, E_CONTRACT, E_NOT_FOUND, OK};
use dkrt_svc_rt::ImportCore;
use dkrt_svc_samples::events::PoolEvent;
use dkrt_svc_samples::peakwin::{PeakJob, PeakJobs};
use dkrt_svc_samples::samples::Samples;
use dkrt_svc_samples::table::{self, POOL_TABLE};

const CORE: ImportCore = ImportCore;

/// The data of the peak-window jobs: the worker task works on it while the UI task may be inside the owner, so it is a static of the file and not a field of the owner (`PeakJob`: a
/// phase word hands each cell between the tasks).
pub static DK_PEAK_JOBS: PeakJobs = PeakJobs::new();

/// The job-end entry of a peak-window job (the runtime calls it in the UI task): the cell takes the status, and the end goes to the owner as an event, so the window changes under the
/// owner's guard (idle: at once; inside a call: after it).
pub unsafe extern "C" fn peak_done(ctx: *mut u8, status: i32) {
    let event = PeakJob::end(ctx, status);
    let _ = crate::DK_SVC_OWNER.post(event);
}

fn client(h: *mut u8) -> u32 {
    h as usize as u32
}

/// The owner for the duration of `f`; `E_CONTEXT` when there is none (before `init` or after `fini`: the file answers as "no service").
fn owner(f: impl FnOnce(&mut Samples) -> i32) -> i32 {
    crate::DK_SVC_OWNER.with(f).unwrap_or(E_CONTEXT)
}

/// The call `f` on the pool table, or `E_CONTEXT` when the service is not loaded (before `init`, after `fini`). The table is not the owner's: a call answers at once from any task, also while
/// the owner is inside a call (`table.rs`).
fn pool(f: impl FnOnce() -> i32) -> i32 {
    if crate::DK_SVC_OWNER.is_live() {
        f()
    } else {
        E_CONTEXT
    }
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_open(ctx: *mut u8, out: *mut *mut u8) -> i32 {
    if out.is_null() || CORE.module_of(ctx) == 0 {
        return E_CONTRACT;
    }
    let c = owner(|s| s.open_first(&CORE));
    if c <= 0 {
        return if c == 0 { E_NOT_FOUND } else { c };
    }
    *out = c as usize as *mut u8;
    OK
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_add(p: *mut u8, path: *const u8) -> i32 {
    pool(|| table::pool_add(&POOL_TABLE, &CORE, client(p), path))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_clear(p: *mut u8, index: u32) -> i32 {
    pool(|| table::pool_clear(&POOL_TABLE, &CORE, client(p), index))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_status(p: *mut u8, index: u32) -> i32 {
    pool(|| table::pool_status(&POOL_TABLE, &CORE, client(p), index))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_info(p: *mut u8, index: u32, out: *mut u8) -> i32 {
    pool(|| table::pool_info(&POOL_TABLE, &CORE, client(p), index, out))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_path(
    p: *mut u8,
    index: u32,
    buf: *mut u8,
    cap: u32,
) -> i32 {
    pool(|| table::pool_path(&POOL_TABLE, &CORE, client(p), index, buf, cap))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_set(p: *mut u8, index: u32, path: *const u8) -> i32 {
    pool(|| table::pool_set(&POOL_TABLE, &CORE, client(p), index, path))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_peaks(
    p: *mut u8,
    index: u32,
    token: u32,
    out: *mut u8,
    capacity: u32,
) -> i32 {
    owner(|s| s.peaks(client(p), index, token, out, capacity))
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_peaks_window(
    p: *mut u8,
    index: u32,
    token: u32,
    first: u32,
    frames: u32,
    columns: u32,
    out: *mut u8,
    capacity: u32,
) -> i32 {
    owner(|s| {
        s.peaks_window(
            &CORE,
            &DK_PEAK_JOBS,
            peak_done,
            client(p),
            index,
            token,
            first,
            frames,
            columns,
            out,
            capacity,
        )
    })
}

/// The slot `pool_done` of the sample-pool adapter: load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn pool_done_fill(req: u32, result: i32) {
    // no owner (after `fini`): the event is dropped, as the adapter does for an empty slot; inside a call of the owner it is queued and runs after it
    let _ = crate::DK_SVC_OWNER.post(PoolEvent::Done { req, result });
}

/// The slot `pool_lost`: the backing was lost, all handles are invalid.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn pool_lost_fill() {
    let _ = crate::DK_SVC_OWNER.post(PoolEvent::Lost);
}
