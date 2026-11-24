//! Stock renames of the current and of other projects.
use crate::*;
use dkrt_svc_project::dkp::{self};

#[test]
fn a_stock_rename_writes_the_new_name_first_and_removes_the_old_file_after() {
    let mut r = Rig::new();
    r.rec(12, b"OLD", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(12);
    // the stock rename keeps the stamp
    r.rec(12, b"NEW", 9);
    r.hook_save(12);
    r.settle();
    assert_eq!(r.f.ops, [('w', path(12, b"NEW")), ('d', path(12, b"OLD"))]);
    let h = dkp::check(&r.f.store[&path(12, b"NEW")], DEV).unwrap();
    assert_eq!(
        (h.counter, &h.name[..h.name_len as usize]),
        (5, &b"NEW"[..])
    );
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_rename_of_the_current_project_with_a_new_stamp_replaces_the_bound_file_unarchived() {
    let mut r = Rig::new();
    r.rec(1, b"A", 9);
    r.f.store.insert(
        path(1, b"A"),
        file_bytes(1, b"A", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(1);
    // the stock rename of the current project gives it a new name and a new stamp
    r.rec(1, b"B", 77);
    r.hook_save(1);
    r.settle();
    assert_eq!(r.f.ops, [('w', path(1, b"B")), ('d', path(1, b"A"))]);
    let h = dkp::check(&r.f.store[&path(1, b"B")], DEV).unwrap();
    assert_eq!((h.stamp, h.counter), (77, 5));
    assert!(archived(&r).is_empty(), "{:?}", archived(&r));
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_file_of_the_slot_with_another_stamp_that_was_not_bound_is_still_archived() {
    let mut r = Rig::new();
    r.rec(1, b"A", 9);
    r.f.store.insert(
        path(1, b"A"),
        file_bytes(1, b"A", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(1);
    let stale = file_bytes(1, b"C", 5, 2, &[(b"test/owner/k", b"c")]);
    r.f.store.insert(path(1, b"C"), stale.clone());
    r.hook_save(1);
    r.put(b"k", b"w");
    r.settle();
    assert_eq!(r.f.store[&archive(b"C", 5)], stale);
    assert!(!r.f.store.contains_key(&path(1, b"C")));
    assert!(r.f.store.contains_key(&path(1, b"A")));
}

#[test]
fn a_cut_between_the_new_name_and_the_removal_leaves_two_files_and_the_higher_counter_wins() {
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 4, &[(b"test/owner/k", b"old")]),
    );
    r.f.store.insert(
        path(12, b"NEW"),
        file_bytes(12, b"NEW", 9, 5, &[(b"test/owner/k", b"new")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"new");
    assert_eq!(
        r.f.ops,
        [('d', path(12, b"OLD"))],
        "the loser is removed, a superseded copy of the same project"
    );
    assert!(r.f.toasts.is_empty());
    // the same with the old name at the higher counter: the file is renamed on load
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 8, &[(b"test/owner/k", b"old")]),
    );
    r.f.store.insert(
        path(12, b"NEW"),
        file_bytes(12, b"NEW", 9, 5, &[(b"test/owner/k", b"new")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"old");
    assert_eq!(
        r.f.ops,
        [
            ('d', path(12, b"NEW")),
            ('w', path(12, b"NEW")),
            ('d', path(12, b"OLD"))
        ]
    );
    assert_eq!(dkp::head_of(&r.f.store[&path(12, b"NEW")]).counter, 9);
    assert_eq!(r.f.store.len(), 1);
}

#[test]
fn a_load_renames_a_file_whose_project_was_renamed_outside_digikit() {
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 2, &[(b"test/owner/k", b"v")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"v");
    assert_eq!(r.f.ops, [('w', path(12, b"NEW")), ('d', path(12, b"OLD"))]);
    assert_eq!(r.f.store.len(), 1);
    assert!(r.f.toasts.is_empty());
}
