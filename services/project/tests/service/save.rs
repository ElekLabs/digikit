//! Saving: the header, the counter, discarded puts, a save into another slot.
use crate::*;
use dkrt_sdk::logk::PROJECT_SAVED;
use dkrt_sdk::OK;
use dkrt_svc_project::dkp::{self};
use dkrt_svc_project::engine::*;

#[test]
fn a_save_writes_the_file_with_the_header_of_the_slot_and_a_counter_that_grows() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.load(12);
    assert_eq!(r.put(b"pool", b"abc"), OK);
    assert_eq!(r.p.status(), ST_DIRTY);
    r.hook_save(12);
    r.settle();
    let p = path(12, b"MY SONG");
    let b = &r.f.store[&p];
    let h = dkp::check(b, DEV).unwrap();
    assert_eq!(
        (h.slot, h.stamp, h.counter, h.device),
        (12, 0x1234_5678, 1, DEV)
    );
    assert_eq!(&h.name[..h.name_len as usize], b"MY SONG");
    assert_eq!(r.f.ops, [('w', p.clone())]);
    assert_eq!(r.f.count(PROJECT_SAVED), 1);
    assert_eq!(r.f.last(PROJECT_SAVED)[..4], [12, h.used, 1, 0]);
    assert_eq!(r.p.status(), ST_FILE);
    // a save with nothing new writes nothing (no card write for the stock save of a project nobody touched)
    let commits = r.f.commits;
    r.hook_save(12);
    r.settle();
    assert_eq!(r.f.commits, commits);
    // a put of the stored value is no change, a changed value is
    assert_eq!(r.put(b"pool", b"abc"), OK);
    assert_eq!(r.p.status(), ST_FILE);
    assert_eq!(r.put(b"pool", b"abd"), OK);
    r.hook_save(12);
    r.settle();
    assert_eq!(dkp::head_of(&r.f.store[&p]).counter, 2);
    assert_eq!(r.f.store.len(), 1);
}

#[test]
fn a_load_without_a_save_discards_the_puts() {
    let mut r = Rig::new();
    r.rec(12, b"A", 1);
    r.rec(13, b"B", 2);
    r.f.store.insert(
        path(12, b"A"),
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"saved")]),
    );
    r.load(12);
    assert_eq!(r.put(b"k", b"unsaved"), OK);
    r.load(13);
    assert_eq!(
        r.f.store[&path(12, b"A")],
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"saved")])
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"saved");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_save_into_another_slot_adopts_the_image_with_no_file_and_dirty() {
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(9, b"COPY", 2);
    r.f.store.insert(
        path(4, b"OLD"),
        file_bytes(4, b"OLD", 1, 1, &[(b"test/owner/k", b"data")]),
    );
    r.load(4);
    r.hook_save(9);
    r.settle();
    assert_eq!(r.p.slot(), 9);
    let head = dkp::check(&r.f.store[&path(9, b"COPY")], DEV).unwrap();
    assert_eq!(
        (head.slot, head.counter),
        (9, 1),
        "written although it was never changed"
    );
    assert_eq!(r.p.status(), ST_FILE);
}
