//! The modules whose effective settings differ when a project loads.
use crate::*;
use dkrt_svc_project::dkp::{self, Image};

/// A project file with entries of several owners: (owner, bare key, value).
pub(crate) fn file_owned(
    slot: u8,
    name: &[u8],
    stamp: u32,
    data: &[(&[u8], &[u8], &[u8])],
) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    for (o, k, v) in data {
        dkp::put(&mut i.0, o, k, v).unwrap();
    }
    dkp::seal(&mut i.0, 1);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

pub(crate) fn changed(r: &mut Rig) -> Vec<String> {
    let mut v: Vec<String> =
        r.f.changed
            .drain(..)
            .map(|o| String::from_utf8(o).unwrap())
            .collect();
    v.sort();
    v
}

#[test]
fn only_the_modules_whose_effective_settings_differ_are_named_when_a_project_loads() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(6, b"TWO", 2);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(
            5,
            b"ONE",
            1,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"2")],
        ),
    );
    r.f.store.insert(
        path(6, b"TWO"),
        file_owned(
            6,
            b"TWO",
            2,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"9")],
        ),
    );
    // the first project: no value in the record, so both modules' values change from their defaults
    r.load(5);
    assert_eq!(changed(&mut r), ["test/a", "test/b"]);
    // the record holds what the last project wrote (a write goes to both)
    r.f.record.insert(b"test/a/k".to_vec(), b"1".to_vec());
    r.f.record.insert(b"test/b/k".to_vec(), b"2".to_vec());
    // the second project holds the same value for a and another for b
    r.load(6);
    assert_eq!(changed(&mut r), ["test/b"], "a reads 1 in both projects");
}

#[test]
fn a_project_without_a_file_changes_only_what_differs_from_the_record() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(7, b"NEW", 3);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(
            5,
            b"ONE",
            1,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"2")],
        ),
    );
    r.f.record.insert(b"test/a/k".to_vec(), b"1".to_vec());
    r.f.record.insert(b"test/b/k".to_vec(), b"2".to_vec());
    r.load(5);
    changed(&mut r);
    // a new project starts from the record: nothing reads another value
    r.load(7);
    assert!(changed(&mut r).is_empty());
    // the record moved on (another project wrote it): the project that held the old value differs now
    r.f.record.insert(b"test/a/k".to_vec(), b"5".to_vec());
    r.load(5);
    assert_eq!(
        changed(&mut r),
        ["test/a"],
        "b equals the record, a does not"
    );
}

#[test]
fn a_key_only_one_project_holds_and_a_cleared_or_unloaded_project_count_too() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(6, b"TWO", 2);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(5, b"ONE", 1, &[(b"test/a", b"k", b"1")]),
    );
    r.f.store.insert(
        path(6, b"TWO"),
        file_owned(6, b"TWO", 2, &[(b"test/c", b"j", b"x")]),
    );
    r.f.record.insert(b"test/a/k".to_vec(), b"0".to_vec());
    r.load(5);
    changed(&mut r);
    // 6 holds c/j only; 5's a/k (1) against the record (0) differs, and c/j against nothing differs
    r.load(6);
    assert_eq!(changed(&mut r), ["test/a", "test/c"]);
    // no current project: every key reads the record again
    r.load(0x80);
    assert_eq!(changed(&mut r), ["test/c"]);
    // clearing the current project empties its image the same way
    r.load(6);
    changed(&mut r);
    r.rec(6, b"NEW PROJECT", 2);
    r.hook_clear(6);
    r.settle();
    assert_eq!(changed(&mut r), ["test/c"]);
}

#[test]
fn every_module_is_named_once_however_many_modules_differ() {
    let mut r = Rig::new();
    // 24 owners with two keys each: more than the table of named owners holds
    let owners: Vec<Vec<u8>> = (0..24)
        .map(|i| format!("test/own{i:02}").into_bytes())
        .collect();
    let keys: Vec<(&[u8], &[u8], &[u8])> = owners
        .iter()
        .flat_map(|o| {
            [
                (o.as_slice(), &b"a"[..], &b"1"[..]),
                (o.as_slice(), &b"b"[..], &b"2"[..]),
            ]
        })
        .collect();
    r.rec(9, b"BIG", 0x99);
    r.f.store
        .insert(path(9, b"BIG"), file_owned(9, b"BIG", 0x99, &keys));
    r.load(9);
    let named = changed(&mut r);
    assert_eq!(named.len(), 24, "each owner once: {named:?}");
    let mut once = named.clone();
    once.dedup();
    assert_eq!(once, named);
}
