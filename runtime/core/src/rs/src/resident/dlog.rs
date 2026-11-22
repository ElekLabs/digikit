//! `DlogFns`: the resident log. `sink_drop`, `sink_on`, `sink_present` and `span_end` are the owner's own `extern "C"` functions.
use crate::dlog;
use dkrt_abi::module::Ctx;
use dkrt_abi::resident::{Group, DlogFns};

unsafe extern "C" fn ev(kind: u16, flags: u16, args: *const u32) {
    dlog::ev(kind, flags, *(args as *const [u32; 5]))
}
extern "C" fn current_source() -> u32 {
    dlog::current_source() as u32
}
extern "C" fn enter_module(idx: u32, phase: u32) -> u32 {
    dlog::enter_module(idx as usize, phase as u8) as u32
}
extern "C" fn leave_module(prev: u32) {
    dlog::leave_module(prev as u16)
}
unsafe extern "C" fn call_init(idx: u32, f: unsafe extern "C" fn(*const Ctx) -> i32, ctx: *const Ctx) -> i32 {
    dlog::call_init(idx as usize, f, ctx)
}
unsafe extern "C" fn call_fini(idx: u32, f: unsafe extern "C" fn(*const Ctx), ctx: *const Ctx) {
    dlog::call_fini(idx as usize, f, ctx)
}
unsafe extern "C" fn release_sinks(idx: u32) {
    dlog::release_sinks(idx as usize)
}

unsafe extern "C" fn span_t0(t0: *mut u32) -> i32 {
    match dlog::span_t0() {
        Some(t) => {
            *t0 = t;
            0
        }
        None => -1,
    }
}
extern "C" fn span_owner() -> u32 {
    dlog::span_owner() as u32
}

pub(super) static FNS: Group<DlogFns> = Group::new(DlogFns {
    ev,
    current_source,
    enter_module,
    leave_module,
    call_init,
    call_fini,
    release_sinks,
    sink_drop: dlog::sink_drop,
    sink_on: dlog::sink_on,
    sink_present: dlog::sink_present,
    span_t0,
    span_end: dlog::span_end,
    span_owner,
});
