//! Host tests of the `samples` service over `FakeCore`: the life of a module's track table (admit, claim, load, release) and the ticks of the service, driven through the
//! part traits of the owner, as the part tables call them. The service has no record kind, so its parts are the DSP-section ones and the tick. Each test makes its own owner.
//! The state machine of the tracks and the table writes have unit tests in `src/tracks`, `src/writes` and the peak windows in `src/peakwin`.
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{DspPart, Provider, Reentry, Ticker};
use dkrt_abi::service::{tick_phase, Claim};
use dkrt_sdk::*;
use dkrt_svc_rt::{Events, Owner, Post};
use dkrt_svc_samples::events::PoolEvent;
use dkrt_svc_samples::pool::{self, Pool};
use dkrt_svc_samples::samples::Samples;
use dkrt_test_support::FakeCore;

// the core imports of the owner cell's guard (no runtime is linked)
mod stubs;

/// The 32-byte track-table record of a DSP section (little-endian words): kind 2, the module's local id, the offsets of its records and control sets.
fn bank(local: u32) -> [u8; 32] {
    let mut b = [0u8; 32];
    for (i, w) in [2u32, local, 0x100, 0x200].iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    b
}

fn job(bank: Option<&[u8; 32]>) -> Job {
    let mut j = Job::empty();
    if let Some(b) = bank {
        j.bank = b.as_ptr();
    }
    j
}

fn claim(s: &mut Samples, core: &FakeCore, rec: usize, j: &Job) {
    let mut c = Claim {
        rec,
        job: j,
        base: 0x1000,
        byte_shift: 0,
        state_at: 0,
    };
    DspPart::<FakeCore>::claim(s, core, &mut c);
}

fn admit(s: &mut Samples, core: &FakeCore, j: &Job) -> Result<(), i32> {
    DspPart::<FakeCore>::admit(s, core, j)
}

#[test]
fn a_module_without_a_track_table_is_not_the_services_business() {
    let core = FakeCore::new();
    let mut s = Samples::new(Pool::none());
    assert_eq!(admit(&mut s, &core, &job(None)), Ok(()));
    let mut other = bank(1);
    other[0] = 3; // another record kind
    assert_eq!(admit(&mut s, &core, &job(Some(&other))), Ok(()));
    claim(&mut s, &core, 0, &job(None));
    assert!(!s.has_table(0));
    // a section of another version is not checked here
    // SAFETY: an all-zero section is a plain value
    let sec: DspSec = unsafe { std::mem::zeroed() };
    assert_eq!(
        DspPart::<FakeCore>::check_sections(&mut s, &core, &sec, &mut job(None)),
        Ok(())
    );
    DspPart::<FakeCore>::release(&mut s, &core, 0);
    assert!(core.calls().is_empty());
}

#[test]
fn a_track_table_is_admitted_claimed_loaded_and_released() {
    let core = FakeCore::new();
    let mut s = Samples::new(Pool::none());
    let (b0, b1, b2) = (bank(1), bank(1), bank(1));
    assert_eq!(admit(&mut s, &core, &job(Some(&b0))), Ok(()));
    claim(&mut s, &core, 4, &job(Some(&b0)));
    assert!(s.has_table(4));
    claim(&mut s, &core, 5, &job(Some(&b1)));
    assert!(s.has_table(5));
    // both client slots are taken: the next table is refused before the loader reserves anything
    assert_eq!(admit(&mut s, &core, &job(Some(&b2))), Err(E_TABLE_FULL));
    assert!(
        !s.table_is_live(4),
        "a claimed table writes nothing before its image is in"
    );
    DspPart::<FakeCore>::loaded(&mut s, &core, 4);
    assert!(s.table_is_live(4) && !s.table_is_live(5));
    // a release gives the slot back; a module that holds none is left alone
    DspPart::<FakeCore>::release(&mut s, &core, 4);
    assert!(!s.has_table(4) && s.has_table(5));
    assert_eq!(admit(&mut s, &core, &job(Some(&b2))), Ok(()));
    DspPart::<FakeCore>::release(&mut s, &core, 4);
    DspPart::<FakeCore>::release(&mut s, &core, 5);
    assert!(!s.has_table(5));
}

#[test]
fn two_owners_do_not_share_their_tracks() {
    let core = FakeCore::new();
    let mut a = Samples::new(Pool::none());
    let b = Samples::new(Pool::none());
    let table = bank(1);
    claim(&mut a, &core, 4, &job(Some(&table)));
    assert!(a.has_table(4) && !b.has_table(4));
}

std::thread_local! {
    /// How often the adapter's `dkpool_tick` ran on this thread.
    static ADAPTER_TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

unsafe extern "C" fn counting_tick() {
    ADAPTER_TICKS.with(|n| n.set(n.get() + 1));
}

#[test]
fn the_adapter_tick_is_early_only_and_needs_the_pool_adapter() {
    ADAPTER_TICKS.with(|n| n.set(0));
    let ticks = || ADAPTER_TICKS.with(|n| n.get());
    // no sample-pool adapter in the image: nothing runs
    Samples::pre_tick_with(&Pool::none(), tick_phase::EARLY);
    assert_eq!(ticks(), 0);
    // with the adapter: the early phase runs the adapter's tick, a late pass is not the service's
    let pool = Pool::new(Some(pool::PoolFns {
        tick: counting_tick,
        ..pool::refusing()
    }));
    Samples::pre_tick_with(&pool, tick_phase::LATE);
    assert_eq!(ticks(), 0);
    Samples::pre_tick_with(&pool, tick_phase::EARLY);
    assert_eq!(ticks(), 1);
}

#[test]
fn the_tick_needs_the_pool_adapter_and_runs_no_adapter_call_itself() {
    let core = FakeCore::new();
    ADAPTER_TICKS.with(|n| n.set(0));
    let mut s = Samples::new(Pool::none());
    Ticker::<FakeCore>::tick(&mut s, &core, tick_phase::EARLY);
    // with the adapter the tick steps the tracks, and the adapter's own tick is the pre-tick step's (before the owner is entered), not this call's
    let mut s = Samples::new(Pool::new(Some(pool::PoolFns {
        tick: counting_tick,
        ..pool::refusing()
    })));
    Ticker::<FakeCore>::tick(&mut s, &core, tick_phase::EARLY);
    Ticker::<FakeCore>::tick(&mut s, &core, tick_phase::LATE);
    assert_eq!(ADAPTER_TICKS.with(|n| n.get()), 0);
    // with no track table a pass writes nothing to the DSP
    assert!(core.calls().is_empty());
    assert!(core.link.borrow().t.wire.is_empty());
}

#[test]
fn the_owner_made_in_a_dirty_slot_is_the_owner_made_by_new() {
    let core = FakeCore::new();
    let mut slot = std::mem::MaybeUninit::<Samples>::uninit();
    // a dirty slot: `start_in` must not read what was there
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    Provider::<FakeCore>::start_in(&core, &mut slot).expect("start");
    // SAFETY: `start_in` made it
    let mut s = unsafe { slot.assume_init() };
    let (b0, b1, b2) = (bank(1), bank(1), bank(1));
    claim(&mut s, &core, 4, &job(Some(&b0)));
    claim(&mut s, &core, 5, &job(Some(&b1)));
    assert!(s.has_table(4) && s.has_table(5) && !s.table_is_live(4));
    assert_eq!(admit(&mut s, &core, &job(Some(&b2))), Err(E_TABLE_FULL));
    DspPart::<FakeCore>::release(&mut s, &core, 4);
    assert_eq!(admit(&mut s, &core, &job(Some(&b2))), Ok(()));
}

type Cell = Owner<Samples, TestEvents, { <Samples as Reentry<FakeCore>>::QUEUE }>;

std::thread_local! {
    /// The owner under test, for the adapter's tick to post to as the service file's event slots do through the cell.
    static OWNER: std::cell::Cell<*const Cell> = const { std::cell::Cell::new(std::ptr::null()) };
    static CORE: std::cell::Cell<*const FakeCore> = const { std::cell::Cell::new(std::ptr::null()) };
}

/// What the file's `service!` hands the owner: the event reaches the object with the file's core.
struct TestEvents;

impl Events<Samples> for TestEvents {
    type Event = PoolEvent;
    fn apply(s: &mut Samples, e: PoolEvent) {
        // SAFETY: the test keeps the core alive across the tick
        Reentry::<FakeCore>::apply(s, unsafe { &*CORE.with(|c| c.get()) }, e)
    }
}

unsafe extern "C" fn losing_tick() {
    // SAFETY: the test keeps the owner cell alive across the call; it mirrors the slots `pool_done` and `pool_lost` of the service file
    let cell = unsafe { &*OWNER.with(|o| o.get()) };
    // the adapter's tick runs before the owner is entered: with the owner idle both events run at once, in the order they fire
    assert_eq!(cell.post(PoolEvent::Done { req: 9, result: 0 }), Post::Ran);
    assert_eq!(cell.post(PoolEvent::Lost), Post::Ran);
}

#[test]
fn the_adapters_events_apply_before_the_tick_of_the_pass() {
    let core = FakeCore::new();
    let cell: Cell = Owner::new();
    CORE.with(|c| c.set(&core as *const FakeCore));
    let pool = Pool::new(Some(pool::PoolFns {
        tick: losing_tick,
        ..pool::refusing()
    }));
    cell.start(|slot| {
        slot.write(Samples::new(pool));
        Ok(())
    })
    .unwrap();
    OWNER.with(|o| o.set(&cell as *const Cell));
    // the pre-tick step, as the table's tick function calls it outside the guard
    Samples::pre_tick_with(&pool, tick_phase::EARLY);
    assert_eq!(
        core.calls().len(),
        1,
        "the lost-pool event applied (and was logged) before the tick began"
    );
    assert_eq!(cell.refused(), 0);
    // the tick of the same pass then runs with those events behind it
    cell.run((), |s| {
        Ticker::<FakeCore>::tick(s, &core, tick_phase::EARLY)
    });
    assert_eq!(cell.refused(), 0);
    let table = bank(1);
    cell.with(|s| {
        claim(s, &core, 4, &job(Some(&table)));
        assert!(s.has_table(4), "the owner is intact after the events");
    });
}
