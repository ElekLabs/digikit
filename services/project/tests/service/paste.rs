//! Pasting a project from one slot to another.
use crate::*;
use dkrt_svc_project::dkp::{self};

#[test]
fn a_paste_copies_the_source_file_to_the_destination_with_its_own_header() {
    let mut r = Rig::new();
    // slot 2 holds the source; slot 50 was pasted over (stamp and name are the source's)
    r.f.store.insert(
        path(2, b"SONG"),
        file_bytes(2, b"SONG", 0x42, 6, &[(b"test/owner/k", b"data")]),
    );
    // a stale file in the destination slot
    r.f.store.insert(
        path(50, b"OLDER"),
        file_bytes(50, b"OLDER", 0x99, 1, &[(b"test/owner/k", b"stale")]),
    );
    r.rec(50, b"SONG", 0x42);
    r.hook_paste(50);
    r.settle();
    let h = dkp::check(&r.f.store[&path(50, b"SONG")], DEV).unwrap();
    assert_eq!((h.slot, h.stamp, h.counter), (50, 0x42, 1));
    assert!(
        r.f.store.contains_key(&path(2, b"SONG")),
        "the source stays"
    );
    assert!(r.f.store.contains_key(&archive(b"OLDER", 0x99)));
    assert!(!r.f.store.contains_key(&path(50, b"OLDER")));
    // loading the copy gives the source's data
    r.load(50);
    assert_eq!(r.get(b"k"), b"data");
    assert!(r.f.toasts.is_empty());
}

#[test]
fn a_paste_with_no_source_file_copies_nothing() {
    let mut r = Rig::new();
    r.f.store.insert(
        path(2, b"SONG"),
        file_bytes(2, b"SONG", 0x41, 1, &[(b"test/owner/k", b"d")]),
    );
    r.rec(50, b"SONG", 0x42);
    let before = r.f.store.clone();
    r.hook_paste(50);
    r.settle();
    assert_eq!(r.f.store, before);
}
