//! A failed load is remembered and tried again.
use crate::*;
use dkrt_sdk::logk::{PROJECT_DEFERRED, PROJECT_IGNORED, PROJECT_LOADED, PROJECT_SAVED};
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use dkrt_svc_project::dkp::Ignore;
use dkrt_svc_project::engine::*;

/// A rig whose project 5 has a file, and whose first load of it is refused by the store with a status that is no busy answer.
pub(crate) fn failed_load_of_5() -> (Rig, Vec<u8>) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = 1;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.load(5);
    assert_eq!(r.p.slot(), -1);
    (r, file)
}

#[test]
fn a_save_into_the_slot_whose_load_failed_keeps_its_file_and_loads_it_again() {
    let (mut r, file) = failed_load_of_5();
    // the scope is off while the load is failed
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    assert_eq!(r.p.scope_remove(OWNER, b"k"), E_SET_OFF);
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // the firmware stays on slot 5 and saves into it: nothing is archived, nothing is written
    r.hook_save(5);
    r.settle();
    assert_eq!(
        r.f.store[&path(5, b"PROJ")],
        file,
        "the file stays in its slot"
    );
    assert!(archived(&r).is_empty());
    assert!(r.f.ops.is_empty(), "no write and no removal");
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [5, Ignore::SaveSkipped as u32, 0, 0, 0],
        "the skipped save is logged, once"
    );
    assert_eq!(
        r.f.count(PROJECT_IGNORED),
        2,
        "the refused load, the skipped save"
    );
    // the save asked for the load again, and it bound the slot
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_setter_call_loads_the_slot_whose_load_failed_again() {
    let (mut r, _) = failed_load_of_5();
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
}

#[test]
fn a_setter_call_to_another_slot_leaves_the_failed_load_until_that_load_ends() {
    let (mut r, file) = failed_load_of_5();
    r.rec(6, b"OTHER", 0x66);
    // the load of slot 6 waits for a live operation: slot 5 stays failed, and its save still loads instead of archiving
    r.f.live_op = 1;
    r.hook_slot(6);
    r.pass();
    r.hook_save(5);
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty());
    assert_eq!(
        r.p.slot(),
        6,
        "the setter's load ran, the save wrote nothing"
    );
}

#[test]
fn a_load_that_stays_busy_fails_and_a_save_into_the_slot_loads_it_again() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = u32::MAX;
    r.hook_slot(5);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    r.f.refuse_list = 0;
    r.hook_save(5);
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty() && r.f.ops.is_empty());
    assert_eq!(r.p.slot(), 5);
}

#[test]
fn a_busy_read_while_probing_a_file_is_no_missing_file_and_binds_no_empty_project() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.f.store.insert(
        path(12, b"MY SONG"),
        file_bytes(
            12,
            b"MY SONG",
            0x1234_5678,
            3,
            &[(b"test/owner/pool", b"abc")],
        ),
    );
    r.f.busy_reads.insert(path(12, b"MY SONG"), 1);
    r.load(12);
    assert_eq!(r.p.slot(), -1, "not bound while the store is busy");
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(r.p.slot(), 12);
    assert_eq!(r.get(b"pool"), b"abc");
}
