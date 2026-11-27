//! The project that is current at the start and the setter call.
use crate::*;
use dkrt_sdk::logk::PROJECT_LOADED;
use dkrt_svc_project::dkp::{self};
use dkrt_svc_project::engine::*;

/// The files of the archive folder.
pub(crate) fn archived(r: &Rig) -> Vec<Vec<u8>> {
    r.f.store
        .keys()
        .filter(|k| k.starts_with(b"projects/archive/"))
        .cloned()
        .collect()
}

#[test]
fn a_project_that_was_current_at_the_start_is_bound_without_a_setter_call_and_its_save_writes_it() {
    let mut r = Rig::new();
    r.rec(5, b"MY SONG", 0x0000_0777);
    let file = file_bytes(5, b"MY SONG", 0x777, 2, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"MY SONG"), file);
    r.f.firmware_slot = Some(5);
    // no hook ran: the stock setter is not called for a project that was already current
    r.settle();
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.p.status(), ST_FILE);
    // the first save writes the bound project under its own slot: it is no save-as, nothing is archived
    r.put(b"k", b"changed");
    r.hook_save(5);
    r.settle();
    assert!(archived(&r).is_empty(), "{:?}", archived(&r));
    let head = dkp::check(&r.f.store[&path(5, b"MY SONG")], DEV).unwrap();
    assert_eq!((head.slot, head.stamp, head.counter), (5, 0x777, 3));
    assert_eq!(r.get(b"k"), b"changed");
}

#[test]
fn a_setter_call_before_the_first_read_names_the_current_project_and_the_read_stays_off() {
    let mut r = Rig::new();
    r.rec(5, b"A", 1);
    r.rec(6, b"B", 2);
    r.f.firmware_slot = Some(5);
    r.hook_slot(6);
    r.settle();
    assert_eq!(
        r.p.slot(),
        6,
        "the setter's slot wins over the read at the start"
    );
    assert_eq!(r.f.count(PROJECT_LOADED), 1);
}

#[test]
fn a_project_whose_record_is_not_listed_yet_is_bound_when_the_record_appears() {
    let mut r = Rig::new();
    r.f.firmware_slot = Some(5);
    r.settle();
    assert_eq!(r.p.slot(), -1, "no record of the slot yet");
    r.rec(5, b"LATE", 9);
    r.settle();
    assert_eq!(r.p.slot(), 5);
}
