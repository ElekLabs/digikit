//! The C entry points of the samples service file: the nine imports a module calls (`dk_sample_pool_*`, the handle first) and the two slots the sample-pool
//! adapter calls. The handle a module holds is its client number, as a pointer (never null: a client is 1 or more); each call resolves it inside the service.
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::{E_CONTRACT, E_NOT_FOUND, OK};
use dkrt_svc_rt::ImportCore;
use dkrt_svc_samples::{peakwin, pool, tracks};

const CORE: ImportCore = ImportCore;

fn client(h: *mut u8) -> u32 {
    h as usize as u32
}

/// `setup` of the file (the first step of `init`): the adapter's calls from `sample_pool_info`. Without the adapter the file does not load (`requires = core/sample-pool` of its module.toml, so its users are refused with a missing need); `None` here is a host build, where there is no way to call the adapter.
pub unsafe fn setup(core: &ImportCore) -> i32 {
    pool::set(pool::from_info(core));
    OK
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_open(ctx: *mut u8, out: *mut *mut u8) -> i32 {
    if out.is_null() || CORE.module_of(ctx) == 0 {
        return E_CONTRACT;
    }
    let c = tracks::open_first(&CORE);
    if c <= 0 {
        return if c == 0 { E_NOT_FOUND } else { c };
    }
    *out = c as usize as *mut u8;
    OK
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_add(p: *mut u8, path: *const u8) -> i32 {
    tracks::pool_add(&CORE, client(p), path)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_clear(p: *mut u8, index: u32) -> i32 {
    tracks::pool_clear(&CORE, client(p), index)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_status(p: *mut u8, index: u32) -> i32 {
    tracks::status(client(p), index)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_info(p: *mut u8, index: u32, out: *mut u8) -> i32 {
    tracks::info(client(p), index, out)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_path(p: *mut u8, index: u32, buf: *mut u8, cap: u32) -> i32 {
    tracks::path(&CORE, client(p), index, buf, cap)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_set(p: *mut u8, index: u32, path: *const u8) -> i32 {
    tracks::pool_set(&CORE, client(p), index, path)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_sample_pool_peaks(p: *mut u8, index: u32, token: u32, out: *mut u8, capacity: u32) -> i32 {
    tracks::peaks(client(p), index, token, out, capacity)
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
    peakwin::window(&CORE, client(p), index, token, first, frames, columns, out, capacity)
}

/// The slot `pool_done` of the sample-pool adapter: load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn pool_done_fill(req: u32, result: i32) {
    tracks::pool_done(req, result)
}

/// The slot `pool_lost`: the backing was lost, all handles are invalid.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn pool_lost_fill() {
    tracks::pool_lost(&CORE)
}
