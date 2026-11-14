//! `samples` service: per-track stereo samples and the peak window.
pub mod peakwin;
pub mod tracks;
pub mod dsp;
pub mod records;

use crate::core_api::{client, give, own_rec};
use dkrt_sdk::*;

// ---- the sample pool --------------------------------------------------------------------------------------------------------

pub unsafe extern "C" fn dk_sample_pool_open(ctx: *mut u8, out: *mut *mut u8) -> i32 {
    if out.is_null() || own_rec(ctx).is_none() {
        return E_CONTRACT;
    }
    give(out, tracks::open_first())
}

pub unsafe extern "C" fn dk_sample_pool_add(p: *mut u8, path: *const u8) -> i32 {
    tracks::pool_add(client(p), path)
}

pub unsafe extern "C" fn dk_sample_pool_clear(p: *mut u8, index: u32) -> i32 {
    tracks::pool_clear(client(p), index)
}

pub unsafe extern "C" fn dk_sample_pool_status(p: *mut u8, index: u32) -> i32 {
    tracks::status(client(p), index)
}

pub unsafe extern "C" fn dk_sample_pool_info(p: *mut u8, index: u32, out: *mut u8) -> i32 {
    tracks::info(client(p), index, out)
}

pub unsafe extern "C" fn dk_sample_pool_path(p: *mut u8, index: u32, buf: *mut u8, cap: u32) -> i32 {
    tracks::path(client(p), index, buf, cap)
}

pub unsafe extern "C" fn dk_sample_pool_set(p: *mut u8, index: u32, path: *const u8) -> i32 {
    tracks::pool_set(client(p), index, path)
}

pub unsafe extern "C" fn dk_sample_pool_peaks(p: *mut u8, index: u32, token: u32, out: *mut u8, capacity: u32) -> i32 {
    tracks::peaks(client(p), index, token, out, capacity)
}

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
    peakwin::window(client(p), index, token, first, frames, columns, out, capacity)
}
