//! The span producers (`entry::tick_all`, `tick::phase`, `tick::poll_all`) over a fake clock and a fake emit: nothing is read or emitted without a span sink
//! and a clock. A span carries its producer's owner, the first clock reading and the elapsed cycles across a wrap. The sink channel is global state. The
//! tests share the lock of `cleanup_tests`.
use crate::fake_platform::test_arena::TestArena;
use crate::dlog::{self, Span, SINK_SPAN};
use crate::entry::{self, Ctx};
use crate::state::{DKRT_POLLS, POLLS};
use crate::tick::{phase, poll_all};
use core::ptr::addr_of_mut;
use dkrt_sdk::{E_CONTRACT, E_NOMEM, OK};
use std::vec::Vec;

/// The fake clock: each read returns `NOW` and then adds `STEP`, so a span of one read pair is `STEP` cycles whatever the measured code does.
static mut NOW: u32 = 0;
static mut STEP: u32 = 0;
static mut CLOCK_CALLS: u32 = 0;
/// What the fake emit got, as `(owner, cycles, start_tick)`.
static mut EMITS: Vec<(u32, u32, u32)> = Vec::new();
/// How many times the fake module tick ran.
static mut TICKED: u32 = 0;

unsafe extern "C" fn fake_clock() -> u32 {
    CLOCK_CALLS += 1;
    let v = NOW;
    NOW = NOW.wrapping_add(STEP);
    v
}

unsafe extern "C" fn fake_emit(s: *const Span) {
    (*addr_of_mut!(EMITS)).push(((*s).owner, (*s).cycles, (*s).start_tick));
}

unsafe extern "C" fn fake_poll() {}

unsafe extern "C" fn fake_tick(_: *const Ctx) {
    TICKED += 1;
}

fn p(f: usize) -> *mut u8 {
    f as *mut u8
}

/// Start a case: the clock at `now` stepping by `step`, nothing recorded, no sink, no poll and no tick.
unsafe fn reset(now: u32, step: u32) {
    NOW = now;
    STEP = step;
    CLOCK_CALLS = 0;
    TICKED = 0;
    (*addr_of_mut!(EMITS)).clear();
    *crate::st() = core::mem::zeroed();
    *addr_of_mut!(crate::state::DKRT_MENU_OBJS) = [0; crate::state::MAX_MODULES];
    let mut i = 0;
    while i < POLLS {
        (*addr_of_mut!(DKRT_POLLS))[i] = 0;
        i += 1;
    }
    // Other loader tests use synthetic tick addresses for both content modules and providers.
    for rec in 0..crate::state::REC_MAX {
        entry::publish_tick(rec, 0);
    }
    // A test before may have left the channel owned by another module (E_BUSY): clear it, not ask.
    dlog::test_reset_sinks();
}

/// Run `f` as code of module 0 (a sink is set from a module's code).
unsafe fn in_module<R>(f: impl FnOnce() -> R) -> R {
    let prev = dlog::enter_module(0, dlog::PHASE_INIT);
    let r = f();
    dlog::leave_module(prev);
    r
}

fn emits() -> Vec<(u32, u32, u32)> {
    unsafe { (*addr_of_mut!(EMITS)).clone() }
}

#[test]
fn no_sink_means_no_clock_read_and_no_span() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        (*addr_of_mut!(DKRT_POLLS))[0] = fake_poll as *const () as usize;
        phase(0);
        poll_all();
        entry::tick_all();
        assert_eq!(CLOCK_CALLS, 0);
        assert!(emits().is_empty());
        assert_eq!(dlog::span_t0(), None);
    }
}

#[test]
fn an_emit_without_a_clock_measures_nothing() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        assert_eq!(in_module(|| dlog::dk_sink_set(SINK_SPAN, p(fake_emit as *const () as usize))), OK);
        (*addr_of_mut!(DKRT_POLLS))[0] = fake_poll as *const () as usize;
        phase(0);
        poll_all();
        entry::tick_all();
        assert_eq!(CLOCK_CALLS, 0);
        assert!(emits().is_empty());
        assert_eq!(dlog::span_t0(), None);
        in_module(|| dlog::dk_sink_set(SINK_SPAN, core::ptr::null_mut()));
    }
}

/// The span of a module tick (`entry::tick_all` measures the call with the record index, the emit after `leave_module`): one span per call with the owner and the clock's
/// difference, a wrap of the counter included. The 32-bit `DKRT_TICKS` table cannot hold a fake's address on a 64-bit host, so `tick_all` itself is not run here.
#[test]
fn spanned_emits_one_record_per_call_with_its_owner_and_start() {
    let _g = TestArena::lock();
    unsafe {
        reset(u32::MAX - 2, 10);
        assert_eq!(in_module(|| dlog::dk_span_sink_set(p(fake_emit as *const () as usize), p(fake_clock as *const () as usize))), OK);
        let mut n = 0;
        for rec in [0u32, 2, 5] {
            let r = dlog::spanned(rec, || {
                fake_tick(core::ptr::null());
                7
            });
            assert_eq!(r, 7);
            n += 1;
            assert_eq!(TICKED, n);
            assert_eq!(CLOCK_CALLS, 2 * n);
            assert_eq!(emits().len(), n as usize);
        }
        // the first span wrapped the counter, the later ones did not
        assert_eq!(emits(), [(0, 10, u32::MAX - 2), (2, 10, 17), (5, 10, 37)]);
        in_module(|| dlog::dk_span_sink_set(core::ptr::null_mut(), core::ptr::null_mut()));
        let before = CLOCK_CALLS;
        dlog::spanned(9, || fake_tick(core::ptr::null()));
        assert_eq!(CLOCK_CALLS, before);
        assert_eq!(emits().len(), 3);
    }
}

#[test]
fn a_phase_and_each_poll_carry_their_own_owner() {
    let _g = TestArena::lock();
    unsafe {
        reset(u32::MAX - 2, 10);
        assert_eq!(in_module(|| dlog::dk_span_sink_set(p(fake_emit as *const () as usize), p(fake_clock as *const () as usize))), OK);
        phase(1);
        // The clock wrapped during the phase. The record keeps the original reading and the elapsed cycles.
        assert_eq!(emits(), [(0x101, 10, u32::MAX - 2)]);
        (*addr_of_mut!(DKRT_POLLS))[0] = fake_poll as *const () as usize;
        (*addr_of_mut!(DKRT_POLLS))[1] = fake_poll as *const () as usize;
        poll_all();
        assert_eq!(emits(), [(0x101, 10, u32::MAX - 2), (0x200, 10, 17), (0x201, 10, 37)]);
        assert_eq!(CLOCK_CALLS, 6);
        in_module(|| dlog::dk_span_sink_set(core::ptr::null_mut(), core::ptr::null_mut()));
        (*addr_of_mut!(DKRT_POLLS))[0] = 0;
        (*addr_of_mut!(DKRT_POLLS))[1] = 0;
    }
}

#[test]
fn releasing_the_sinks_clears_the_clock() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        assert_eq!(in_module(|| dlog::dk_span_sink_set(p(fake_emit as *const () as usize), p(fake_clock as *const () as usize))), OK);
        assert!(dlog::span_t0().is_some());
        dlog::release_sinks(0);
        assert_eq!(dlog::span_t0(), None);
        // an emit set again without a clock does not measure: the clock is gone, not only the channel
        assert_eq!(in_module(|| dlog::dk_sink_set(SINK_SPAN, p(fake_emit as *const () as usize))), OK);
        let calls = CLOCK_CALLS;
        assert_eq!(dlog::span_t0(), None);
        assert_eq!(CLOCK_CALLS, calls);
        in_module(|| dlog::dk_sink_set(SINK_SPAN, core::ptr::null_mut()));
    }
}

#[test]
fn dk_sink_set_on_the_span_channel_clears_the_clock() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        assert_eq!(in_module(|| dlog::dk_span_sink_set(p(fake_emit as *const () as usize), p(fake_clock as *const () as usize))), OK);
        assert!(dlog::span_t0().is_some());
        assert_eq!(in_module(|| dlog::dk_sink_set(SINK_SPAN, core::ptr::null_mut())), OK);
        let calls = CLOCK_CALLS;
        assert_eq!(dlog::span_t0(), None);
        (*addr_of_mut!(DKRT_POLLS))[0] = fake_poll as *const () as usize;
        phase(0);
        poll_all();
        entry::tick_all();
        assert_eq!(CLOCK_CALLS, calls);
        assert!(emits().is_empty());
        // The clock is gone too: a sink set again through the old API still measures nothing.
        assert_eq!(in_module(|| dlog::dk_sink_set(SINK_SPAN, p(fake_emit as *const () as usize))), OK);
        assert_eq!(dlog::span_t0(), None);
        in_module(|| dlog::dk_sink_set(SINK_SPAN, core::ptr::null_mut()));
    }
}

/// `dkrt_poll_add` records the owner of a slot: the module record whose loaded range holds the function, the runtime marker for any other address.
#[test]
fn a_poll_slot_records_the_module_that_registered_it() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        let s = crate::st();
        s.modules[1].used = 1;
        s.modules[1].base = 0x1000;
        s.modules[1].size = 0x100;
        s.modules[3].used = 1;
        s.modules[3].base = 0x2000;
        s.modules[3].size = 0x100;
        assert_eq!(crate::dkrt_poll_add(0x1010), OK);
        assert_eq!(crate::dkrt_poll_add(0x9000), OK);
        assert_eq!(crate::dkrt_poll_add(0x20ff), OK);
        let mut f = 0u32;
        assert_eq!(crate::report::debug_poll(0, &mut f), 2, "record 1 gives 2");
        assert_eq!(f, 0x1010);
        assert_eq!(crate::report::debug_poll(1, &mut f), 0xff, "outside every module: the runtime marker");
        assert_eq!(f, 0x9000);
        assert_eq!(crate::report::debug_poll(2, &mut f), 4);
        assert_eq!(f, 0x20ff);
        assert_eq!(crate::report::debug_poll(3, &mut f), 0, "an empty slot");
        assert_eq!(crate::report::debug_poll(4, core::ptr::null_mut()), 0, "past the table");
        // adding a function that is in the table keeps its slot and owner
        assert_eq!(crate::dkrt_poll_add(0x1010), OK);
        assert_eq!(crate::report::debug_poll(0, core::ptr::null_mut()), 2);
        // a module that is released takes its polls with it; the others stay
        crate::api::poll_drop_range(0x1000, 0x100);
        assert_eq!(crate::report::debug_poll(0, &mut f), 0);
        assert_eq!(crate::report::debug_poll(2, &mut f), 4);
        // removing one frees the slot; a new function reuses it with its own owner
        crate::dkrt_poll_remove(0x9000);
        assert_eq!(crate::report::debug_poll(1, &mut f), 0);
        assert_eq!(crate::dkrt_poll_add(0x1020), OK);
        assert_eq!(crate::report::debug_poll(0, &mut f), 2);
        assert_eq!(f, 0x1020);
        reset(0, 5);
    }
}

static mut CALLED: Vec<u32> = Vec::new();
unsafe extern "C" fn observer_a(owner: u32) {
    (*core::ptr::addr_of_mut!(CALLED)).push(owner);
}
unsafe extern "C" fn observer_b(owner: u32) {
    (*core::ptr::addr_of_mut!(CALLED)).push(owner + 100);
}

/// The unload observers: a table of four, adding one twice keeps one slot, an observer inside the unloaded module is dropped and not called, and the
/// others are called in table order with the owner.
#[test]
fn unload_observers_are_called_with_the_owner_and_an_own_observer_is_dropped() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        (*core::ptr::addr_of_mut!(CALLED)).clear();
        let a = observer_a as *const () as usize;
        let b = observer_b as *const () as usize;
        assert_eq!(crate::dkrt_unload_observer_add(0), E_CONTRACT);
        assert_eq!(crate::dkrt_unload_observer_add(a), OK);
        assert_eq!(crate::dkrt_unload_observer_add(a), OK, "in the table already");
        assert_eq!(crate::dkrt_unload_observer_add(b), OK);
        crate::api::notify_unload(3, 0, 0);
        assert_eq!(*core::ptr::addr_of!(CALLED), [3, 103]);
        // an observer in the range of the unloaded module (a fake address: it must not be called) is dropped
        assert_eq!(crate::dkrt_unload_observer_add(0x5010), OK);
        (*core::ptr::addr_of_mut!(CALLED)).clear();
        crate::api::notify_unload(4, 0x5000, 0x100);
        assert_eq!(*core::ptr::addr_of!(CALLED), [4, 104]);
        (*core::ptr::addr_of_mut!(CALLED)).clear();
        crate::api::notify_unload(5, 0x5000, 0x100);
        assert_eq!(*core::ptr::addr_of!(CALLED), [5, 105]);
        crate::dkrt_unload_observer_remove(a);
        // four slots
        for i in 1..4usize {
            assert_eq!(crate::dkrt_unload_observer_add(0x7000 + i), OK);
        }
        assert_eq!(crate::dkrt_unload_observer_add(0x7100), E_NOMEM);
        crate::dkrt_unload_observer_remove(b);
        assert_eq!(crate::dkrt_unload_observer_add(0x7100), OK);
        for f in [0x7001usize, 0x7002, 0x7003, 0x7100] {
            crate::dkrt_unload_observer_remove(f);
        }
        assert_eq!(*core::ptr::addr_of!(crate::state::DKRT_UNLOAD_OBSERVERS), [0; 4]);
        reset(0, 5);
    }
}

/// `dkrt_ctx_owner` gives the record of a live module context and refuses any other pointer.
#[test]
fn a_module_context_names_its_owner() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        crate::entry::bind(2);
        // the host's addresses are 64 bits wide: take the address of the context itself, not `ctx_addr`'s u32
        let ctx = core::ptr::addr_of!(entry::DKRT_CTXS[2]) as usize;
        assert_eq!(crate::dkrt_ctx_owner(ctx), 2);
        assert_eq!(crate::dkrt_ctx_owner(ctx + 1), E_CONTRACT);
        assert_eq!(crate::dkrt_ctx_owner(0), E_CONTRACT);
        assert_eq!(crate::dkrt_ctx_owner(core::ptr::addr_of!(entry::DKRT_CTXS[3]) as usize), E_CONTRACT, "record 3 is not loaded");
        crate::entry::unbind(2);
        // a service file's record has a context too, but no owner id is given out for it
        crate::entry::bind(crate::MAX_MODULES);
        assert_eq!(crate::dkrt_ctx_owner(core::ptr::addr_of!(entry::DKRT_CTXS[crate::MAX_MODULES]) as usize), E_CONTRACT);
        crate::entry::unbind(crate::MAX_MODULES);
        reset(0, 5);
    }
}

/// `dkrt_menu_built` counts a menu for the module record that holds the fill function, else the descriptor; an address in no module counts for none.
#[test]
fn a_menu_is_counted_for_the_module_that_holds_its_code_or_descriptor() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        let s = crate::st();
        for (rec, base) in [(1usize, 0x1000u32), (3, 0x2000)] {
            s.modules[rec].used = 1;
            s.modules[rec].base = base;
            s.modules[rec].size = 0x100;
        }
        let n = |rec: usize| core::ptr::read_volatile(core::ptr::addr_of!(crate::state::DKRT_MENU_OBJS[rec]));
        crate::dkrt_menu_built(0x1010, 0x2010);
        assert_eq!((n(1), n(3)), (1, 0), "the fill function decides");
        crate::dkrt_menu_built(0x9000, 0x2010);
        assert_eq!((n(1), n(3)), (1, 1), "no module holds the fill function: the descriptor does");
        crate::dkrt_menu_built(0x9000, 0x9100);
        assert_eq!((n(1), n(3)), (1, 1), "a menu of the firmware image counts for no module");
        reset(0, 5);
    }
}

/// The DIGIKIT menu holds a module while its SETTINGS page is open (`dkrt_menu_built` with the module's `menu` function) and gives the hold back when the page is left
/// (`dkrt_menu_release`): the job refuses the module while the count is above 0 (`menu_built`, the fact `reload::refusal` turns into `E_RELOAD_MENUS`, and the read before the teardown),
/// a release lets it through, releases never take the count below 0, and a load into the record starts at 0, so a release that arrives after it changes nothing.
#[test]
fn an_open_page_holds_the_module_and_the_release_lets_it_go() {
    use crate::reload::{Ops, RealOps};
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        let s = crate::st();
        for (rec, base) in [(1usize, 0x1000u32), (3, 0x2000)] {
            s.modules[rec].used = 1;
            s.modules[rec].base = base;
            s.modules[rec].size = 0x100;
        }
        let n = |rec: usize| core::ptr::read_volatile(core::ptr::addr_of!(crate::state::DKRT_MENU_OBJS[rec]));
        let mut ops = RealOps;
        crate::dkrt_menu_built(0x1020, 0); // the page opens: the module's menu function is the code address
        assert!(ops.menu_built(1) && !ops.menu_built(3), "held: the job refuses module 1 only");
        crate::dkrt_menu_release(0x1020, 0); // the page is left
        assert!(!ops.menu_built(1), "released: the job goes on");
        // a menu object the module built itself and the page hold together: both must go
        crate::dkrt_menu_built(0x1010, 0x2000);
        crate::dkrt_menu_built(0x1020, 0);
        crate::dkrt_menu_release(0x1020, 0);
        assert_eq!(n(1), 1, "the module's own menu object still holds it");
        // releases past 0 stay at 0; an address in no module counts for nothing
        crate::dkrt_menu_release(0x1020, 0);
        crate::dkrt_menu_release(0x1020, 0);
        crate::dkrt_menu_release(0x9000, 0x9100);
        assert_eq!((n(1), n(3)), (0, 0), "the count stops at 0");
        // a load into the record reset the count to 0: the release of the old page does not go below
        crate::dkrt_menu_built(0x2020, 0);
        crate::state::DKRT_MENU_OBJS[3] = 0; // `set_menu` at a load
        crate::dkrt_menu_release(0x2020, 0);
        assert_eq!(n(3), 0);
        reset(0, 5);
    }
}

/// The live job reads the count of its own record again before the teardown (`RealOps::menu_built`): a menu built for another module does not end it, one for its module does,
/// a record number past the table is no menu, and the count does not wrap after many menus.
#[test]
fn the_job_reads_the_menu_count_of_its_own_record() {
    use crate::reload::{Ops, RealOps};
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        let s = crate::st();
        for (rec, base) in [(1usize, 0x1000u32), (3, 0x2000)] {
            s.modules[rec].used = 1;
            s.modules[rec].base = base;
            s.modules[rec].size = 0x100;
        }
        let mut ops = RealOps;
        assert!(!ops.menu_built(1) && !ops.menu_built(3), "no menu yet");
        crate::dkrt_menu_built(0x2010, 0);
        assert!(!ops.menu_built(1), "a menu of module 3 is no menu of module 1");
        assert!(ops.menu_built(3));
        assert!(!ops.menu_built(crate::MAX_MODULES), "a record past the table has no menu");
        for _ in 0..300 {
            crate::dkrt_menu_built(0x1010, 0);
        }
        assert!(ops.menu_built(1), "the count stays at its top and does not wrap to 0");
        reset(0, 5);
    }
}

/// `dkrt_phase` says what the running module code is.
#[test]
fn the_phase_is_that_of_the_running_module_code() {
    let _g = TestArena::lock();
    unsafe {
        reset(0, 5);
        assert_eq!(crate::dkrt_phase(), 0);
        let prev = dlog::enter_module(0, dlog::PHASE_INIT);
        assert_eq!(crate::dkrt_phase(), 1);
        dlog::leave_module(prev);
        let prev = dlog::enter_module(0, dlog::PHASE_TICK);
        assert_eq!(crate::dkrt_phase(), 2);
        dlog::leave_module(prev);
        assert_eq!(crate::dkrt_phase(), 0);
    }
}
