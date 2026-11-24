use super::*;
use dkrt_test_support::FakeCore;
use std::boxed::Box;

fn tbl() -> &'static PoolCell {
    Box::leak(Box::new(PoolCell::new(crate::table::PoolTable::new())))
}

fn tracks() -> Box<Tracks> {
    Box::new(Tracks::new())
}

#[test]
fn a_table_takes_the_first_free_client_slot_and_a_full_set_refuses() {
    let mut t = tracks();
    assert!(t.can_reserve() && !t.has(4));
    assert!(t.reserve(&FakeCore::new(), &*tbl(), 4, 1, 0x1000, 0x2000));
    assert!(t.has(4) && !t.has(5));
    assert!(t.reserve(&FakeCore::new(), &*tbl(), 5, 1, 0x3000, 0x4000));
    assert!(!t.can_reserve());
    assert!(
        !t.reserve(&FakeCore::new(), &*tbl(), 6, 1, 0, 0),
        "both slots are taken"
    );
    assert_eq!(t.writes.table(0), (0x1000, 0x2000));
    assert_eq!(t.writes.table(1), (0x3000, 0x4000));
}

#[test]
fn only_the_table_of_the_loaded_module_goes_live() {
    let mut t = tracks();
    t.reserve(&FakeCore::new(), &*tbl(), 4, 1, 0x1000, 0x2000);
    t.reserve(&FakeCore::new(), &*tbl(), 5, 1, 0x3000, 0x4000);
    assert!(
        !t.is_live(4) && !t.is_live(5),
        "a claimed table writes nothing yet"
    );
    t.ready(4);
    assert!(t.is_live(4) && !t.is_live(5));
    t.ready(9);
    assert!(
        !t.is_live(5),
        "a module without a table is not the service's business"
    );
}

#[test]
fn a_release_frees_the_slot_of_the_module_and_leaves_the_other() {
    let core = FakeCore::new();
    let pool = Pool::none();
    let mut t = tracks();
    t.reserve(&FakeCore::new(), &*tbl(), 4, 1, 0x1000, 0x2000);
    t.reserve(&FakeCore::new(), &*tbl(), 5, 1, 0x3000, 0x4000);
    t.ready(4);
    assert_eq!(t.release(&core, &pool, 4, |_| {}), 0b01);
    assert!(!t.has(4) && t.has(5) && !t.is_live(4));
    assert!(t.can_reserve());
    assert_eq!(
        t.release(&core, &pool, 4, |_| {}),
        0,
        "a module that holds none is left alone"
    );
    // the freed slot is the next one a table takes, and it starts closed
    assert!(t.reserve(&FakeCore::new(), &*tbl(), 6, 2, 0x5000, 0x6000));
    assert!(t.has(6) && !t.is_live(6));
}

#[test]
fn a_release_calls_the_client_hook_for_each_slot_of_the_module_in_the_walk() {
    let core = FakeCore::new();
    let pool = Pool::none();
    let mut t = tracks();
    t.reserve(&FakeCore::new(), &*tbl(), 4, 1, 0x1000, 0x2000);
    t.reserve(&FakeCore::new(), &*tbl(), 4, 2, 0x3000, 0x4000);
    let mut seen = std::vec::Vec::new();
    let freed = t.release(&core, &pool, 4, |c| seen.push(c));
    assert_eq!(seen, [0, 1]);
    assert_eq!(freed, 0b11);
}

#[test]
fn the_handle_of_a_table_is_found_by_the_calling_module_and_the_local_id() {
    let core = FakeCore::new();
    let mut t = tracks();
    t.reserve(&FakeCore::new(), &*tbl(), 4, 3, 0x1000, 0x2000);
    t.reserve(&FakeCore::new(), &*tbl(), 5, 2, 0x3000, 0x4000);
    assert_eq!(t.tracks(&core, 3), E_NOT_FOUND, "no module runs");
    core.source.set(5); // module record 4
    assert_eq!(t.tracks(&core, 3), 1);
    assert_eq!(
        t.tracks(&core, 2),
        E_NOT_FOUND,
        "that id is the other module's"
    );
    assert_eq!(t.open_first(&core), 1);
    core.source.set(6); // module record 5
    assert_eq!(t.open_first(&core), 2);
    core.source.set(7);
    assert_eq!(t.open_first(&core), E_NOT_FOUND);
}

#[test]
fn the_state_made_in_place_is_the_state_made_by_new() {
    let mut slot = MaybeUninit::<Tracks>::uninit();
    // a dirty slot: `init` must not read what was there
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    Tracks::init(&mut slot);
    // SAFETY: `init` made it
    let mut t = unsafe { slot.assume_init() };
    assert!(t.can_reserve() && !t.has(0));
    assert!(t.reserve(&FakeCore::new(), &*tbl(), 0, 1, 0x1000, 0x2000));
    assert_eq!(t.client(1), Ok(0));
    assert_eq!(t.client(2), Err(E_NOT_FOUND));
    assert_eq!(t.client(0), Err(E_NOT_FOUND));
}
