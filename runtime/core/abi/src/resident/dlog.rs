//! `DlogFns`: the resident log (`dlog.rs`): events, the source of the running module code, the sinks.
use crate::module::Ctx;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DlogFns {
    /// Append an event: its kind, its flags and five words (`args` points at them).
    pub ev: unsafe extern "C" fn(kind: u16, flags: u16, args: *const u32),
    /// Record index + 1 of the module code that runs now; 0 outside one.
    pub current_source: extern "C" fn() -> u32,
    /// Module code of record `idx` is about to run in `phase`: the previous source and phase for `leave_module`.
    pub enter_module: extern "C" fn(idx: u32, phase: u32) -> u32,
    pub leave_module: extern "C" fn(prev: u32),
    /// Call a module's `init` with its source set.
    pub call_init: unsafe extern "C" fn(
        idx: u32,
        f: unsafe extern "C" fn(*const Ctx) -> i32,
        ctx: *const Ctx,
    ) -> i32,
    /// Call a module's `fini` with its source set.
    pub call_fini:
        unsafe extern "C" fn(idx: u32, f: unsafe extern "C" fn(*const Ctx), ctx: *const Ctx),
    /// Drop the sinks that record `idx` set.
    pub release_sinks: unsafe extern "C" fn(idx: u32),
    /// Drop the sinks whose callback lies in `lo..hi`.
    pub sink_drop: unsafe extern "C" fn(lo: u32, hi: u32),
    pub sink_on: extern "C" fn(channel: u32) -> bool,
    pub sink_present: extern "C" fn() -> bool,
    /// Start a span: 0 and the clock's value in `*t0`, or negative when no span is measured now.
    pub span_t0: unsafe extern "C" fn(t0: *mut u32) -> i32,
    /// End the span of `owner` that started at `t0`.
    pub span_end: unsafe extern "C" fn(owner: u32, t0: u32),
    /// Record index + 1 of the module that owns the span channel; 0 when none does.
    pub span_owner: extern "C" fn() -> u32,
}
