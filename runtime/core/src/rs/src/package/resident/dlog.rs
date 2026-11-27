//! The resident log: events, the source of the running module code, the sinks.
use super::table;
use dkrt_abi::module::Ctx;
pub(crate) use dkrt_abi::log::{INFO, PHASE_CALLBACK, PHASE_INIT, PHASE_TICK, SINK_LOG};

/// Append an event.
#[inline(always)]
pub(crate) unsafe fn ev(kind: u16, flags: u16, a: [u32; 5]) {
    (table().dlog.ev)(kind, flags, a.as_ptr())
}

/// The source index of the module whose code runs now (record index + 1); 0 outside one.
#[inline(always)]
pub(crate) fn current_source() -> u8 {
    (table().dlog.current_source)() as u8
}

/// Module code is about to run in `phase`: the previous source and phase for `leave_module`.
#[inline(always)]
pub(crate) fn enter_module(idx: usize, phase: u8) -> u16 {
    (table().dlog.enter_module)(idx as u32, phase as u32) as u16
}

#[inline(always)]
pub(crate) fn leave_module(prev: u16) {
    (table().dlog.leave_module)(prev as u32)
}

/// Call a module's `init` (record `idx`, context `ctx`) with its source set.
#[inline(always)]
pub(crate) unsafe fn call_init(idx: usize, f: unsafe extern "C" fn(*const Ctx) -> i32, ctx: *const Ctx) -> i32 {
    (table().dlog.call_init)(idx as u32, f, ctx)
}

#[inline(always)]
pub(crate) unsafe fn call_fini(idx: usize, f: unsafe extern "C" fn(*const Ctx), ctx: *const Ctx) {
    (table().dlog.call_fini)(idx as u32, f, ctx)
}

#[inline(always)]
pub(crate) unsafe fn release_sinks(idx: usize) {
    (table().dlog.release_sinks)(idx as u32)
}

#[inline(always)]
pub(crate) unsafe fn sink_drop(lo: u32, hi: u32) {
    (table().dlog.sink_drop)(lo, hi)
}

#[inline(always)]
pub(crate) fn sink_on(channel: u32) -> bool {
    (table().dlog.sink_on)(channel)
}

#[inline(always)]
pub(crate) fn sink_present() -> bool {
    (table().dlog.sink_present)()
}

/// Does anything take the log's events (the legacy sink or the `SINK_LOG` channel)? Producers of optional, frequent events (`LIFECYCLE`) test this first: they pay these two
/// tests while nothing listens and build no event.
#[inline(always)]
pub(crate) fn log_on() -> bool {
    sink_present() || sink_on(SINK_LOG)
}

/// Start a span: the clock's value, `None` when no span is measured now.
#[inline(always)]
pub(crate) fn span_t0() -> Option<u32> {
    let mut t = 0u32;
    if unsafe { (table().dlog.span_t0)(&mut t) } == 0 {
        Some(t)
    } else {
        None
    }
}

/// End the span of `owner` that started at `t0`.
#[inline(always)]
pub(crate) unsafe fn span_end(owner: u32, t0: u32) {
    (table().dlog.span_end)(owner, t0)
}

/// The source index of the module that owns the span channel (record index + 1; 0: none does).
#[inline(always)]
pub(crate) fn span_owner() -> u8 {
    (table().dlog.span_owner)() as u8
}
