//! The owner's lifecycle over a `FakeCore`: the state is made in place from zero bytes, `start` opens the host and sets the settings overlay, `stop` takes it away, a failed
//! `open` leaves the core untouched.
use core::mem::MaybeUninit;
use dkrt_abi::provider::Provider;
use dkrt_svc_project::engine::{Pending, ProjectService};
use dkrt_svc_project::project::Project;
use dkrt_test_support::FakeCore;

mod host;
use host::{TestHost, OPEN_FAILS, TABLE};

type Owner = Project<TestHost>;

/// `start_in` writes zero bytes for the whole state; that is the initial state only when a zeroed engine acts as `ProjectService::new()` does: both are driven through the
/// same project change and must show the same slot and status at each step. (The padding of `new()` is not zero, so the bytes are not compared.)
#[test]
fn a_zeroed_engine_acts_as_a_new_one() {
    let mut zeroed = Box::new(MaybeUninit::<ProjectService>::zeroed());
    // SAFETY: the contract of `Host` and of `start_in`: zero bytes are a valid initial state; this test checks it by behaviour
    let zeroed = unsafe { zeroed.assume_init_mut() };
    let mut fresh = Box::new(ProjectService::new());
    let mut env = TestHost;
    // the tick takes the record and clears it: one record for each engine
    let change = || Pending {
        slot_val: 3,
        slot_flag: 1,
        ..Pending::new()
    };
    for step in 0..6 {
        let (for_zeroed, for_fresh) = if step == 0 {
            (change(), change())
        } else {
            (Pending::new(), Pending::new())
        };
        zeroed.tick(&mut env, &for_zeroed);
        fresh.tick(&mut env, &for_fresh);
        assert_eq!(zeroed.slot(), fresh.slot(), "slot at step {step}");
        assert_eq!(zeroed.status(), fresh.status(), "status at step {step}");
        assert_eq!(zeroed.current(), fresh.current(), "current at step {step}");
    }
    assert_eq!(fresh.slot(), 3);
}

#[test]
fn start_sets_the_overlay_once_and_stop_takes_it_away() {
    let core = FakeCore::new();
    assert!(core.settings_overlay.get().is_null());
    let mut slot = MaybeUninit::<Owner>::uninit();
    Provider::start_in(&core, &mut slot).expect("start");
    assert_eq!(core.settings_overlay.get(), &TABLE as *const _);
    // SAFETY: `start_in` made it
    let owner = unsafe { slot.assume_init_mut() };
    assert_eq!(
        owner.engine().slot(),
        -1,
        "no project is current before the first tick"
    );
    Provider::stop(owner, &core);
    assert!(core.settings_overlay.get().is_null());
}

#[test]
fn a_failed_open_is_the_status_of_start_and_sets_no_overlay() {
    let core = FakeCore::new();
    OPEN_FAILS.with(|f| f.set(-73));
    let mut slot = MaybeUninit::<Owner>::uninit();
    let r = <Owner as Provider<FakeCore>>::start_in(&core, &mut slot);
    OPEN_FAILS.with(|f| f.set(0));
    assert_eq!(r, Err(-73));
    assert!(core.settings_overlay.get().is_null());
}
