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
    t[w::LIST_STOCK] = 0x4700_0000;
    t[w::ROW_SIZE] = 64;
    t[w::O_POS] = 48;
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

/// The four list patches get 4 bytes per listed type (a gap row is not listed) and the addresses of the service's list buffer, and `start` leaves the buffer holding the order.
#[test]
fn start_builds_the_list_and_gives_the_patches_their_bytes() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let mut t = table(1);
    t[w::COUNT] = 3;
    t[w::ENTRIES] = 2;
    t[w::LIST_STOCK] = 0x4700_0000;
    t[w::ROW_SIZE] = 64;
    t[w::O_POS] = 48;
    let core = core_with(Some(t), 1);
    // seven stock types in order 0 .. 6; the rows: position 2, a gap row, the end
    for i in 0..7u32 {
        core.mem.borrow_mut().insert(0x4700_0000 + 4 * i, i);
    }
    for (k, p) in [2u32, 0xffff_ffff, 255].iter().enumerate() {
        core.mem
            .borrow_mut()
            .insert(0x4200_0000 + 64 * k as u32 + 48, *p);
    }
    let m = <Machines as Provider<FakeCore>>::start(&core).unwrap();
    assert_eq!(
        m.list_bytes(),
        9 * 4,
        "seven stock types and two listed rows"
    );
    assert_eq!(m.list_end(), m.list_start() + 9 * 4);
    let list = unsafe { *core::ptr::addr_of!(dkrt_svc_machines::list::DKRT_MF_LIST) };
    assert_eq!(&list[..9], [0, 1, 7, 2, 3, 4, 5, 6, 9]);
}
