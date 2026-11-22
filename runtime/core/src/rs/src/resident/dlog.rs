//! `DlogFns`: the resident log.
use crate::dlog;
use dkrt_abi::module::Ctx;
use dkrt_abi::resident::DlogFns;

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
unsafe extern "C" fn sink_drop(lo: u32, hi: u32) {
    dlog::sink_drop(lo, hi)
}
extern "C" fn sink_on(channel: u32) -> bool {
    dlog::sink_on(channel)
}
extern "C" fn sink_present() -> bool {
    dlog::sink_present()
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
unsafe extern "C" fn span_end(owner: u32, t0: u32) {
    dlog::span_end(owner, t0)
}
extern "C" fn span_owner() -> u32 {
    dlog::span_owner() as u32
}

pub(super) static FNS: DlogFns =
    DlogFns { ev, current_source, enter_module, leave_module, call_init, call_fini, release_sinks, sink_drop, sink_on, sink_present, span_t0, span_end, span_owner };
