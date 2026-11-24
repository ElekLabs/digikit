use super::*;
extern crate std;
use crate::{ImportCore, Reentry};
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

fn put<T>(v: T) -> impl FnOnce(&mut MaybeUninit<T>) -> Result<(), Status> {
    move |slot| {
        slot.write(v);
        Ok(())
    }
}

#[test]
fn with_gives_nothing_before_start_and_after_stop() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(7)).is_ok());
    assert_eq!(o.with(|v| *v), Some(7));
    assert_eq!(
        o.with(|v| {
            *v += 1;
            *v
        }),
        Some(8)
    );
    let mut seen = 0;
    assert!(o.stop(|v| seen = *v));
    assert_eq!(seen, 8);
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(!o.stop(|_| unreachable!()));
    // and it can start again after a stop
    assert!(o.start(put(1)).is_ok());
}

#[test]
fn a_second_start_is_refused_without_making_an_instance() {
    let o: Owner<u32> = Owner::new();
    let at = o.start(put(1)).unwrap();
    assert_eq!(o.instance(), at);
    let mut made = false;
    let r = o.start(|_| {
        made = true;
        Ok(())
    });
    assert_eq!(r, Err(dkrt_sdk::E_LOADED));
    assert!(!made);
    assert_eq!(o.with(|v| *v), Some(1));
}

#[test]
fn a_start_that_fails_leaves_the_cell_empty() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.start(|_| Err(dkrt_sdk::E_NOMEM)), Err(dkrt_sdk::E_NOMEM));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(3)).is_ok());
}

#[test]
fn a_nested_with_finds_nothing_instead_of_a_second_reference() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let inner = o.with(|_| o.with(|v| *v));
    assert_eq!(inner, Some(None));
    assert_eq!(o.with(|v| *v), Some(5), "the cell is usable again after");
}

#[test]
fn stop_inside_with_is_refused_and_the_state_stays() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let r = o.with(|v| {
        let stopped = o.stop(|_| unreachable!());
        *v += 1;
        (stopped, *v)
    });
    assert_eq!(r, Some((false, 6)));
    assert_eq!(o.with(|v| *v), Some(6), "the instance is still live");
}

#[test]
fn start_inside_with_is_refused_and_the_state_stays() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let r = o.with(|v| {
        let started = o.start(|_| unreachable!());
        (started, *v)
    });
    assert_eq!(r, Some((Err(dkrt_sdk::E_LOADED), 5)));
    assert_eq!(o.with(|v| *v), Some(5));
}

#[test]
fn start_inside_stop_is_refused_and_the_cell_ends_empty() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let mut inner = None;
    assert!(o.stop(|_| inner = Some(o.start(put(9)))));
    assert_eq!(inner, Some(Err(dkrt_sdk::E_LOADED)));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(1)).is_ok(), "a start after the stop works");
}

#[test]
fn start_inside_start_is_refused() {
    let o: Owner<u32> = Owner::new();
    let mut inner = None;
    let r = o.start(|slot| {
        inner = Some(o.start(put(9)));
        slot.write(4);
        Ok(())
    });
    assert!(r.is_ok());
    assert_eq!(inner, Some(Err(dkrt_sdk::E_LOADED)));
    assert_eq!(o.with(|v| *v), Some(4));
}

#[test]
fn with_and_stop_inside_a_stop_are_refused() {
    // (inside a start nothing is live yet, so `with` and `stop` refuse there without the transition guard: not tested here)
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let mut seen = None;
    assert!(o.stop(|_| seen = Some((o.with(|v| *v), o.stop(|_| ()), o.instance().is_null()))));
    assert_eq!(seen, Some((None, false, true)));
}

static DROPS: AtomicU32 = AtomicU32::new(0);
struct Counted;
impl Drop for Counted {
    fn drop(&mut self) {
        DROPS.fetch_add(1, SeqCst);
    }
}

#[test]
fn stop_drops_the_value_once_in_place() {
    let o: Owner<Counted> = Owner::new();
    o.start(put(Counted)).unwrap();
    let at = o.instance();
    let before = DROPS.load(SeqCst);
    assert!(o.stop(|v| assert_eq!(v as *mut Counted, at)));
    assert_eq!(DROPS.load(SeqCst), before + 1);
    assert!(!o.stop(|_| ()));
    assert_eq!(
        DROPS.load(SeqCst),
        before + 1,
        "a second stop drops nothing"
    );
}

// ---- re-entry: events and part calls while the owner is busy ---------------------------------------------------------------------

/// A fake service: its part call posts events and re-enters the owner, as a pool callback does inside `tick`.
struct Fake {
    /// True for the whole of a part call (set by the call itself).
    in_call: bool,
    /// The `in_call` flag as each event found it, and the events in the order they ran.
    ran: [u32; 8],
    saw_in_call: [bool; 8],
    count: usize,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            in_call: false,
            ran: [0; 8],
            saw_in_call: [false; 8],
            count: 0,
        }
    }
}

impl Reentry<ImportCore> for Fake {
    type Event = u32;
    const QUEUE: usize = 2;
    fn apply(&mut self, _core: &ImportCore, event: u32) {
        self.ran[self.count] = event;
        self.saw_in_call[self.count] = self.in_call;
        self.count += 1;
    }
}

struct FakeEvents;

impl Events<Fake> for FakeEvents {
    type Event = u32;
    fn apply(f: &mut Fake, event: u32) {
        f.apply(&ImportCore, event)
    }
}

type FakeOwner = Owner<Fake, FakeEvents, 2>;

fn fake() -> FakeOwner {
    let o = FakeOwner::new();
    o.start(put(Fake::new())).unwrap();
    o
}

#[test]
fn an_event_during_a_part_call_runs_after_it_in_order() {
    let o = fake();
    let r = o.run(0, |f| {
        f.in_call = true;
        assert_eq!(o.post(10), Post::Queued);
        assert_eq!(o.post(11), Post::Queued);
        assert_eq!(f.count, 0, "nothing ran inside the call");
        f.in_call = false;
        7
    });
    assert_eq!(r, 7);
    o.with(|f| {
        assert_eq!(f.count, 2);
        assert_eq!(&f.ran[..2], &[10, 11], "in the order they arrived");
    });
    assert_eq!(o.refused(), 0);
}

#[test]
fn no_second_mut_is_formed_inside_a_part_call() {
    let o = fake();
    let mut entered = false;
    o.run((), |f| {
        f.in_call = true;
        // a nested part call and a nested `with` are refused: their closures never run
        assert_eq!(
            o.run(5, |_| {
                entered = true;
                1
            }),
            5
        );
        assert_eq!(
            o.with(|_| {
                entered = true;
            }),
            None
        );
        // the event is applied after the call, never while `f` is alive
        o.post(1);
        f.in_call = false;
    });
    assert!(!entered);
    assert_eq!(o.refused(), 2, "both nested calls were counted");
    o.with(|f| assert!(!f.saw_in_call[0], "apply ran with the call over"));
}

#[test]
fn a_full_queue_refuses_and_counts() {
    let o = fake();
    o.run((), |_| {
        assert_eq!(o.post(1), Post::Queued);
        assert_eq!(o.post(2), Post::Queued);
        assert_eq!(o.post(3), Post::Full);
    });
    assert_eq!(o.refused(), 1);
    o.with(|f| {
        assert_eq!(f.count, 2);
        assert_eq!(&f.ran[..2], &[1, 2], "the refused one is not in the queue");
    });
}

#[test]
fn an_owner_is_all_zero_bytes_before_start() {
    // an owner with a queue and no function pointer stays `.bss`: no file bytes
    let o = FakeOwner::new();
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &o as *const FakeOwner as *const u8,
            core::mem::size_of::<FakeOwner>(),
        )
    };
    assert!(bytes.iter().all(|b| *b == 0));
}

#[test]
fn a_normal_call_is_unchanged() {
    let o = fake();
    // idle: `post` runs the event at once, `run` and `with` return what the closure returns
    assert_eq!(o.post(4), Post::Ran);
    assert_eq!(o.run(0, |f| f.count as u32), 1);
    assert_eq!(o.with(|f| f.ran[0]), Some(4));
    assert_eq!(o.refused(), 0);
    // not live: refused answer, no event
    let dead = FakeOwner::new();
    assert_eq!(dead.run(9, |_| 1), 9);
    assert_eq!(dead.post(1), Post::NoOwner);
    assert_eq!(dead.refused(), 0);
}

// ---- atomic against preemption: the flags and the queue change with interrupts masked --------------------------------------------

std::thread_local! {
    /// How many masks are open on this thread, and how many were opened in all.
    static MASK_DEPTH: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    static MASKS_TAKEN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    /// An interrupt: runs when the `at`-th mask is given back (the point where an unmasked CPU takes a pending interrupt).
    static ISR: core::cell::RefCell<Option<(u32, Box<dyn FnOnce()>)>> = const { core::cell::RefCell::new(None) };
    static RESTORES: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// What `ImportCore::irq_mask` does in this crate's host tests: the counters (the runtime links the real import).
pub(crate) fn irq_mask() -> u32 {
    MASK_DEPTH.with(|d| d.set(d.get() + 1));
    MASKS_TAKEN.with(|n| n.set(n.get() + 1));
    0x2700
}

/// `ImportCore::irq_restore` in this crate's host tests: the point where an unmasked CPU takes a pending interrupt.
pub(crate) fn irq_restore(_sr: u32) {
    let depth = MASK_DEPTH.with(|d| {
        d.set(d.get() - 1);
        d.get()
    });
    if depth != 0 {
        return;
    }
    let n = RESTORES.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    let fire = ISR.with(|i| {
        let mut i = i.borrow_mut();
        match i.take() {
            Some((at, f)) if at == n => Some(f),
            other => {
                *i = other;
                None
            }
        }
    });
    if let Some(f) = fire {
        f();
    }
}

pub(crate) fn mask_depth() -> u32 {
    MASK_DEPTH.with(|d| d.get())
}

pub(crate) fn masks_taken() -> u32 {
    MASKS_TAKEN.with(|n| n.get())
}

/// Deliver `f` as an interrupt when the owner gives its `at`-th mask back (counted from now).
pub(crate) fn interrupt_at(at: u32, f: impl FnOnce() + 'static) {
    RESTORES.with(|n| n.set(0));
    ISR.with(|i| *i.borrow_mut() = Some((at, Box::new(f))));
}

#[test]
fn every_call_decides_and_ends_with_interrupts_masked() {
    let o = fake();
    for (what, call) in [
        (
            "run",
            &(|| {
                let _ = o.run(0, |_| 1);
            }) as &dyn Fn(),
        ),
        ("with", &|| {
            let _ = o.with(|_| ());
        }),
        ("enter and leave", &|| {
            assert!(!o.enter().is_null());
            o.leave();
        }),
        ("post", &|| {
            let _ = o.post(1);
        }),
    ] {
        let before = masks_taken();
        call();
        assert!(masks_taken() > before, "{what} takes no mask");
        assert_eq!(MASK_DEPTH.with(|d| d.get()), 0, "{what} leaves a mask open");
    }
    // a queued event adds the masks of its own pop; the whole call still ends unmasked
    let before = masks_taken();
    o.run((), |_| {
        let _ = o.post(2);
    });
    assert!(
        masks_taken() >= before + 4,
        "enter, post, pop and the final check"
    );
    assert_eq!(MASK_DEPTH.with(|d| d.get()), 0);
    let started: Owner<u32> = Owner::new();
    let before = masks_taken();
    started.start(put(1)).unwrap();
    assert!(started.stop(|_| ()));
    assert!(
        masks_taken() >= before + 2,
        "start and stop test their flags masked"
    );
    assert_eq!(MASK_DEPTH.with(|d| d.get()), 0);
}

std::thread_local! {
    static CELL: std::cell::Cell<*const FakeOwner> = const { std::cell::Cell::new(core::ptr::null()) };
}

fn isr_cell() -> &'static FakeOwner {
    // SAFETY: the test keeps its owner alive until the interrupt has run
    unsafe { &*CELL.with(|c| c.get()) }
}

#[test]
fn an_interrupt_right_after_the_entry_finds_the_owner_busy() {
    let o = fake();
    CELL.with(|c| c.set(&o));
    let got = std::rc::Rc::new(std::cell::Cell::new((false, Post::NoOwner)));
    let g = got.clone();
    // the first mask given back is the one that made the owner busy: the interrupt arrives before the call's first instruction
    interrupt_at(1, move || {
        let mut ran = false;
        let with = isr_cell().with(|_| ran = true);
        g.set((ran || with.is_some(), isr_cell().post(7)));
    });
    o.run((), |f| {
        assert_eq!(
            f.count, 0,
            "the interrupt's event has not run inside the call"
        )
    });
    assert_eq!(
        got.get(),
        (false, Post::Queued),
        "a call is refused and an event is queued, never run beside the call"
    );
    o.with(|f| {
        assert_eq!(
            &f.ran[..f.count],
            &[7],
            "the event ran once, after the call"
        )
    });
    assert_eq!(o.refused(), 1, "the refused `with`");
}

#[test]
fn an_event_that_arrives_while_the_queue_drains_is_not_lost() {
    let o = fake();
    CELL.with(|c| c.set(&o));
    // two queued events; the interrupt posts a third when the first pop is over, and a fourth when the owner has just become idle
    o.run((), |_| {
        o.post(1);
        o.post(2);
    });
    o.with(|f| assert_eq!(&f.ran[..f.count], &[1, 2]));
    let o = fake();
    CELL.with(|c| c.set(&o));
    let results = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let r = results.clone();
    o.run((), |_| {
        o.post(1);
        o.post(2);
        // masks given back from now on: leave's first pop (1), its second pop (2), the final idle step (3)
        let r2 = r.clone();
        interrupt_at(1, move || r2.borrow_mut().push(isr_cell().post(3)));
    });
    assert_eq!(
        *results.borrow(),
        vec![Post::Queued],
        "busy at the first pop: queued"
    );
    o.with(|f| {
        assert_eq!(
            &f.ran[..f.count],
            &[1, 2, 3],
            "the event queued by the interrupt ran in order"
        )
    });
    // an interrupt right after the owner became idle runs its event itself
    let o = fake();
    CELL.with(|c| c.set(&o));
    let r = results.clone();
    results.borrow_mut().clear();
    o.run((), |_| {
        o.post(1);
        interrupt_at(2, move || r.borrow_mut().push(isr_cell().post(9)));
    });
    assert_eq!(
        *results.borrow(),
        vec![Post::Ran],
        "idle after the queue was found empty: it runs at once"
    );
    o.with(|f| assert_eq!(&f.ran[..f.count], &[1, 9]));
}
