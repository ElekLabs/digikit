//! `ImportRows`: the module imports the resident core serves. The functions that forward to a resident owner (the allocator, the log, the job and I/O queues, the hash,
//! the boot screen) are defined here; the others are the owners' own `extern "C"` functions.
use crate::health::dkrt_blocks;
use crate::report::{debug_peek, debug_poll, debug_setup, debug_span_clock, debug_speed_flags, debug_track_type, device_track_count};
use crate::state::log_at;
use crate::{arena, aux, debug_selected_params, dlog, hash32, intro, io, selected_track, worker};
use core::ptr::null_mut;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::resident::{Group, ImportRows};
use dkrt_sdk::api::*;

extern "C" {
    // Helpers from compiler_builtins, linked into this object.
    fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    fn memset(d: *mut u8, c: i32, n: usize) -> *mut u8;
    fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32;
    fn __muldi3();
    fn __udivdi3();
    fn __umoddi3();
}

unsafe extern "C" fn imp_alloc(size: u32) -> *mut u8 {
    let a = arena::alloc(size);
    if a.is_null() {
        null_mut()
    } else {
        a.ptr()
    }
}
unsafe extern "C" fn imp_free(p: *mut u8) {
    arena::free(DevAddr::of_ptr(p));
}
unsafe extern "C" fn imp_log(level: u32, msg: *const u8, value: u32) {
    log_at(level, msg, value);
}
unsafe extern "C" fn imp_job_post(step: Option<dkrt_sdk::api::JobStepFn>, done: Option<dkrt_sdk::api::JobDoneFn>, arg: *mut u8) -> i32 { worker::post(dlog::current_source(), step, done, arg) }
unsafe extern "C" fn imp_job_cancel(id: u32) -> i32 { worker::cancel(dlog::current_source(), id) }
unsafe extern "C" fn imp_intro_take(frame: Option<dkrt_sdk::api::IntroFrameFn>, done: Option<dkrt_sdk::api::IntroDoneFn>, arg: *mut u8, ticks: u32) -> i32 { intro::take(frame, done, arg, ticks) }
unsafe extern "C" fn imp_io_post(step: Option<dkrt_sdk::api::IoStepFn>, done: Option<dkrt_sdk::api::IoDoneFn>, arg: *mut u8) -> i32 { io::post(dlog::current_source(), step, done, arg) }
unsafe extern "C" fn imp_io_cancel(id: i32) -> i32 { io::cancel(dlog::current_source(), id) }
unsafe extern "C" fn imp_io_wake(id: i32) { io::wake(id) }
unsafe extern "C" fn imp_helper_run(work: Option<dkrt_sdk::api::HelperFn>, arg: *mut u8) -> i32 { aux::module_run(core::mem::transmute::<Option<dkrt_sdk::api::HelperFn>, usize>(work), arg) }
unsafe extern "C" fn imp_helper_wait() -> i32 { aux::module_wait() }
unsafe extern "C" fn imp_hash32_init(state: *mut u8) { core::ptr::write(state as *mut hash32::Hash32, hash32::Hash32::new()) }
unsafe extern "C" fn imp_hash32_update(state: *mut u8, p: *const u8, n: u32) { (*(state as *mut hash32::Hash32)).update(p, n) }
unsafe extern "C" fn imp_hash32_finish(state: *mut u8) -> u32 { core::ptr::read(state as *const hash32::Hash32).finish() | 1 }

/// The address of the function `$f`, which has the type `$t` that `sdk/sdk.toml` gives the import.
macro_rules! row {
    ($t:ty, $f:expr) => {{
        const _: $t = $f;
        $f as *const ()
    }};
}

/// The same for a function only the device links (the firmware's own, or the hooks of the debug module): a host test links none of them, so its row is null.
macro_rules! device_row {
    ($t:ty, $f:expr) => {{
        const _: $t = $f;
        if cfg!(test) {
            core::ptr::null()
        } else {
            $f as *const ()
        }
    }};
}

pub(super) static ROWS: Group<ImportRows> = Group::new(ImportRows {
    memcpy: memcpy as *const (),
    memset: memset as *const (),
    memcmp: memcmp as *const (),
    muldi3: __muldi3 as *const (),
    udivdi3: __udivdi3 as *const (),
    umoddi3: __umoddi3 as *const (),
    dk_alloc: row!(DkAllocFn, imp_alloc),
    dk_free: row!(DkFreeFn, imp_free),
    dk_log: row!(DkLogFn, imp_log),
    dk_log_event: row!(DkLogEventFn, dlog::dk_log_event),
    dk_sink_set: row!(DkSinkSetFn, dlog::dk_sink_set),
    dk_span_sink_set: row!(DkSpanSinkSetFn, dlog::dk_span_sink_set),
    dk_time_blocks: row!(DkTimeBlocksFn, dkrt_blocks),
    dk_fw_alloc: device_row!(DkFwAllocFn, crate::platform::fw_alloc),
    dk_fw_free: device_row!(DkFwFreeFn, crate::platform::fw_free),
    dk_fw_block_read: device_row!(DkFwBlockReadFn, crate::platform::fw_block_read),
    dk_io_post: row!(DkIoPostFn, imp_io_post),
    dk_io_cancel: row!(DkIoCancelFn, imp_io_cancel),
    dk_io_wake: row!(DkIoWakeFn, imp_io_wake),
    dk_helper_run: row!(DkHelperRunFn, imp_helper_run),
    dk_helper_wait: row!(DkHelperWaitFn, imp_helper_wait),
    dk_job_post: row!(DkJobPostFn, imp_job_post),
    dk_job_cancel: row!(DkJobCancelFn, imp_job_cancel),
    dk_hash32_init: row!(DkHash32InitFn, imp_hash32_init),
    dk_hash32_update: row!(DkHash32UpdateFn, imp_hash32_update),
    dk_hash32_finish: row!(DkHash32FinishFn, imp_hash32_finish),
    dk_intro_take: row!(DkIntroTakeFn, imp_intro_take),
    dk_debug_peek: device_row!(DkDebugPeekFn, debug_peek),
    dk_debug_span_clock: device_row!(DkDebugSpanClockFn, debug_span_clock),
    dk_debug_poll: device_row!(DkDebugPollFn, debug_poll),
    dk_debug_track_type: device_row!(DkDebugTrackTypeFn, debug_track_type),
    dk_debug_selected_params: device_row!(DkDebugSelectedParamsFn, debug_selected_params),
    dk_debug_top_view: device_row!(DkDebugTopViewFn, crate::platform::debug_top_view),
    dk_debug_setup: device_row!(DkDebugSetupFn, debug_setup),
    dk_device_track_count: device_row!(DkDeviceTrackCountFn, device_track_count),
    dk_selected_track: device_row!(DkSelectedTrackFn, selected_track),
    dk_debug_log_sink_set: row!(DkDebugLogSinkSetFn, dlog::dk_debug_log_sink_set),
    dk_debug_speed_flags: device_row!(DkDebugSpeedFlagsFn, debug_speed_flags),
});
