//! The owner `Machines` itself: what `start` and `start_in` do (the image table is bound into `DKRT_MF_TAB`, or the file is refused; the state is made where it lives, over
//! whatever the slot held), and what a new owner holds (nothing collected, the gate closed, no action armed, nothing parked).
use core::mem::MaybeUninit;
use dkneeds::keys::name_hash;
use dkrt_abi::provider::Provider;
use dkrt_sdk::E_NO_REGISTRY;
use dkrt_svc_machines::info::{self, w, DKRT_MF_TAB};
use dkrt_svc_machines::machines::Machines;
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

/// `DKRT_MF_TAB` is one table of the crate: the tests that bind it take turns.
static TURN: Mutex<()> = Mutex::new(());

fn table(device: u32) -> Vec<u32> {
    let mut t = vec![0u32; info::WORDS];
    t[w::MAGIC] = info::MAGIC;
    t[w::VERSION] = info::VERSION;
    t[w::DEVICE] = device;
    t[w::ENSURE] = 0x4000_1000;
    t[w::ROWS] = 0x4200_0000;
    t[w::FIRST] = 7;
    t[w::N_CODE] = 0x4000_2000;
    t
}

fn core_with(words: Option<Vec<u32>>, device: u32) -> FakeCore {
    let mut c = FakeCore::new();
    c.device = device;
    if let Some(t) = words {
        c.info_tables.push((name_hash("machines_info"), t));
    }
    c
}

/// What a fresh owner shows from outside.
fn assert_initial(m: &mut Machines) {
    let ph = m.builder().placeholder();
    assert!(!ph.is_null());
    assert_eq!(m.builder().build().nm, 0);
    assert!(m.builder().args().is_none());
    assert_eq!(m.clicks().pending().callback, 0);
    let p = m.parking();
    assert_eq!(
        (p.missing_rows(), p.tries(), p.logged(), p.busy_passes()),
        (0, 0, 0, 0)
    );
    assert!(m.applied().codes().is_empty());
    assert_eq!(m.applied().native_seen(), 0);
}

#[test]
fn a_new_owner_has_nothing_collected_queued_or_parked() {
    let mut m = Box::new(Machines::new());
    assert_initial(&mut m);
}

#[test]
fn start_refuses_an_image_without_a_usable_table_and_binds_a_good_one() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    // no table, another device's table, no `ENSURE` word
    let mut no_ensure = table(2);
    no_ensure[w::ENSURE] = 0;
    for core in [
        core_with(None, 2),
        core_with(Some(table(1)), 2),
        core_with(Some(no_ensure), 2),
    ] {
        assert!(matches!(
            <Machines as Provider<FakeCore>>::start(&core),
            Err(E_NO_REGISTRY)
        ));
        let mut slot = MaybeUninit::<Machines>::uninit();
        assert_eq!(
            <Machines as Provider<FakeCore>>::start_in(&core, &mut slot),
            Err(E_NO_REGISTRY)
        );
    }
    let core = core_with(Some(table(2)), 2);
    let mut owner = <Machines as Provider<FakeCore>>::start(&core).expect("starts");
    assert_initial(&mut owner);
    // SAFETY: the test holds the turn; nothing else reads the table
    let bound = unsafe { *core::ptr::addr_of!(DKRT_MF_TAB) };
    assert_eq!(bound[..], table(2)[..], "the hooks' table is the image's");
}

#[test]
fn start_in_makes_the_state_where_it_lives_over_whatever_the_slot_held() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let core = core_with(Some(table(2)), 2);
    let mut slot = Box::new(MaybeUninit::<Machines>::uninit());
    // a slot that held a former owner's bytes
    // SAFETY: the slot is plain memory of the size of `Machines`
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    assert_eq!(
        <Machines as Provider<FakeCore>>::start_in(&core, &mut slot),
        Ok(())
    );
    // SAFETY: `start_in` made the state in place
    let m = unsafe { slot.assume_init_mut() };
    assert_initial(m);
}
