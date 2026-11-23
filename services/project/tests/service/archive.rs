//! Archiving files of a slot that is reused, cleared or whose name repeats.
use crate::*;
use dkrt_sdk::proto::E_SET_NOKEY;
use dkrt_svc_project::dkp::{self};
use dkrt_svc_project::name;

pub(crate) fn archive(name: &[u8], stamp: u32) -> Vec<u8> {
    name::archive_path(&n16(name), stamp).as_bytes().to_vec()
}

#[test]
fn a_save_into_a_reused_slot_archives_the_old_file_before_the_new_one_is_written() {
    let mut r = Rig::new();
    // slot 20 held another project; it was deleted and a new one saved there (a new stamp)
    let old = file_bytes(20, b"OLD ONE", 0x0000_00aa, 3, &[(b"test/owner/k", b"old")]);
    r.f.store.insert(path(20, b"OLD ONE"), old.clone());
    r.rec(20, b"FRESH", 0x0000_00bb);
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.hook_slot(20);
    r.hook_save(20);
    r.put(b"k", b"mine");
    r.settle();
    // the project that was current went to slot 20 (a save-as), the stale file was archived
    assert_eq!(
        r.f.ops,
        [
            ('w', archive(b"OLD ONE", 0xaa)),
            ('d', path(20, b"OLD ONE")),
            ('w', path(20, b"FRESH"))
        ]
    );
    assert_eq!(
        r.f.store[&archive(b"OLD ONE", 0xaa)],
        old,
        "the archive copy is the old file's bytes"
    );
    let h = dkp::check(&r.f.store[&path(20, b"FRESH")], DEV).unwrap();
    assert_eq!((h.slot, h.stamp, h.counter), (20, 0xbb, 1));
    assert_eq!(r.p.slot(), 20);
}

#[test]
fn a_stock_clear_archives_the_slots_file_and_empties_the_image_when_the_slot_is_current() {
    let mut r = Rig::new();
    r.rec(9, b"DOOMED", 0x77);
    let b = file_bytes(9, b"DOOMED", 0x77, 2, &[(b"test/owner/k", b"v")]);
    r.f.store.insert(path(9, b"DOOMED"), b.clone());
    r.load(9);
    assert_eq!(r.get(b"k"), b"v");
    // the clear keeps the stamp and writes NEW PROJECT as the name
    r.rec(9, b"NEW PROJECT", 0x77);
    r.hook_clear(9);
    r.settle();
    assert_eq!(
        r.f.ops,
        [('w', archive(b"DOOMED", 0x77)), ('d', path(9, b"DOOMED"))]
    );
    assert_eq!(r.f.store[&archive(b"DOOMED", 0x77)], b);
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // a clear of another slot touches only that slot's file
    r.rec(30, b"OTHER", 5);
    r.f.store.insert(
        path(30, b"OTHER"),
        file_bytes(30, b"OTHER", 5, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.ops.clear();
    r.hook_clear(30);
    r.settle();
    assert!(r.f.ops.contains(&('d', path(30, b"OTHER"))));
    assert!(r.f.store.contains_key(&archive(b"OTHER", 5)));
    // the next stock save of the cleared project must not bring the old data back
    r.hook_save(9);
    r.settle();
    assert!(!r.f.store.keys().any(|k| k.starts_with(b"projects/010")));
}

#[test]
fn a_clear_of_several_slots_archives_each_one() {
    let mut r = Rig::new();
    for s in [3u32, 40, 90] {
        r.rec(s, b"P", 100 + s);
        r.f.store.insert(
            path(s, b"P"),
            file_bytes(s as u8, b"P", 100 + s, 1, &[(b"test/owner/k", b"v")]),
        );
        r.hook_clear(s);
    }
    r.settle();
    assert!(!r
        .f
        .store
        .keys()
        .any(|k| k.starts_with(b"projects/0") && !k.starts_with(b"projects/archive")));
    for s in [3u32, 40, 90] {
        assert!(r.f.store.contains_key(&archive(b"P", 100 + s)));
    }
}

#[test]
fn two_archives_of_the_same_name_and_stamp_keep_both_files() {
    let mut r = Rig::new();
    // a paste copies the name and the stamp: two slots hold a project of the same name and stamp (different data)
    r.rec(5, b"PROJ", 0x55);
    r.rec(6, b"PROJ", 0x55);
    let first = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"first")]);
    let second = file_bytes(6, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"second")]);
    r.f.store.insert(path(5, b"PROJ"), first.clone());
    r.f.store.insert(path(6, b"PROJ"), second.clone());
    r.hook_clear(5);
    r.settle();
    r.hook_clear(6);
    r.settle();
    let first_copy = archive(b"PROJ", 0x55);
    let mut second_copy = first_copy.clone();
    second_copy.truncate(second_copy.len() - 4);
    second_copy.extend_from_slice(b"-2.dkp");
    let mut expected = [first_copy.clone(), second_copy.clone()];
    expected.sort();
    assert_eq!(archived(&r), expected);
    assert_eq!(r.f.store[&first_copy], first);
    assert_eq!(r.f.store[&second_copy], second);
    assert!(
        !r.f.store.contains_key(&path(5, b"PROJ")) && !r.f.store.contains_key(&path(6, b"PROJ"))
    );
}

#[test]
fn an_archive_name_with_no_free_suffix_leaves_the_file_where_it_is() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    for copy in 1..=name::ARCHIVE_COPIES_MAX {
        let taken = name::archive_path_copy(&n16(b"PROJ"), 0x55, copy);
        r.f.store
            .insert(taken.as_bytes().to_vec(), b"older".to_vec());
    }
    r.hook_clear(5);
    r.settle();
    assert_eq!(
        r.f.store[&path(5, b"PROJ")],
        file,
        "never delete what was not archived"
    );
    for copy in 1..=name::ARCHIVE_COPIES_MAX {
        let taken = name::archive_path_copy(&n16(b"PROJ"), 0x55, copy);
        assert_eq!(
            r.f.store[taken.as_bytes()],
            b"older",
            "copy {copy} is untouched"
        );
    }
}

#[test]
fn an_archive_candidate_with_the_header_of_the_source_and_a_corrupt_body_is_taken_not_archived_already(
) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 7, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    // the first archive name holds a file with the source's header and a damaged body (a cut or a bad block)
    let mut damaged = file.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    let name = n16(b"PROJ");
    let first = name::archive_path_copy(&name, 0x55, 1).as_bytes().to_vec();
    r.f.store.insert(first.clone(), damaged.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(
        r.f.store[&first], damaged,
        "the damaged file is never replaced"
    );
    let second = name::archive_path_copy(&name, 0x55, 2).as_bytes().to_vec();
    assert_eq!(r.f.store[&second], file, "the source went to the next name");
    assert!(
        !r.f.store.contains_key(&path(5, b"PROJ")),
        "and left its slot"
    );
}

#[test]
fn a_file_that_passes_the_check_is_the_archive_copy_already_and_only_the_source_goes() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 7, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    let name = n16(b"PROJ");
    let first = name::archive_path_copy(&name, 0x55, 1).as_bytes().to_vec();
    r.f.store.insert(first.clone(), file.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(archived(&r), [first], "no second copy");
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}
