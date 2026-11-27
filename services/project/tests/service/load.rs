//! Loading a project: empty, installed, mismatch, damaged files.
use crate::*;
use dkrt_sdk::logk::{PROJECT_IGNORED, PROJECT_LOADED, PROJECT_SAVED};
use dkrt_sdk::proto::E_SET_NOKEY;
use dkrt_svc_project::dkp::{self, Ignore};
use dkrt_svc_project::engine::*;

#[test]
fn a_project_without_a_file_loads_empty_with_no_toast_no_write_and_no_delete() {
    let mut r = Rig::new();
    r.rec(5, b"NEW PROJECT", 0x1111_0005);
    r.load(5);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.f.count(PROJECT_LOADED), 1);
    assert_eq!(&r.f.last(PROJECT_LOADED)[..4], &[5, 0, 0, 0]);
    assert!(r.f.toasts.is_empty() && r.f.ops.is_empty() && r.f.store.is_empty());
    assert!(r.f.changed.is_empty(), "no module settings changed");
    assert_eq!(r.p.status(), 0);
    // the scope is on and empty
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // a stock save of a project nobody put anything into writes nothing either
    r.hook_save(5);
    r.settle();
    assert!(r.f.ops.is_empty() && r.f.store.is_empty());
    assert_eq!(
        r.f.count(PROJECT_SAVED),
        1,
        "SAVED ends the save also when there was nothing to write"
    );
    assert_eq!(&r.f.last(PROJECT_SAVED)[..4], &[5, 0, 0, 0]);
}

#[test]
fn a_file_with_the_stamp_of_the_slot_is_installed_and_the_loaded_event_follows_the_data() {
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
    r.load(12);
    assert_eq!(r.get(b"pool"), b"abc");
    let l = r.f.last(PROJECT_LOADED);
    assert_eq!((l[0], l[3]), (12, 3), "slot and write counter");
    assert!(l[1] > 42);
    assert_eq!(r.p.status(), ST_FILE);
    assert!(
        r.f.ops.is_empty(),
        "a load of a file with the right name writes nothing"
    );
}

#[test]
fn a_stamp_mismatch_gives_one_warning_and_leaves_the_files_alone() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    let old = file_bytes(
        12,
        b"MY SONG",
        0x0bad_f00d,
        1,
        &[(b"test/owner/pool", b"old")],
    );
    r.f.store.insert(path(12, b"MY SONG"), old.clone());
    r.load(12);
    assert_eq!(r.f.toasts, [Ignore::Stamp as u32]);
    assert_eq!(r.f.last(PROJECT_IGNORED)[..2], [12, 4]);
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.f.store[&path(12, b"MY SONG")], old);
    assert!(r.f.ops.is_empty());
    // the image is empty and the project is current
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"pool", &mut out), E_SET_NOKEY);
    assert_eq!(r.p.status(), ST_IGNORED);
}

#[test]
fn a_damaged_file_a_file_of_another_device_and_a_newer_version_are_not_used() {
    for (what, reason) in [
        ("crc", Ignore::Format),
        ("device", Ignore::Device),
        ("version", Ignore::Version),
    ] {
        let mut r = Rig::new();
        r.rec(3, b"X", 7);
        let mut b = file_bytes(3, b"X", 7, 1, &[(b"test/owner/k", b"v")]);
        match what {
            "crc" => {
                let n = b.len();
                b[n - 1] ^= 1;
            }
            // a file of another device or version is whole: its crc32 is right
            "device" => {
                b[6..8].copy_from_slice(&0x7777u16.to_be_bytes());
                dkp::seal(&mut b, 1);
            }
            _ => {
                b[4..6].copy_from_slice(&2u16.to_be_bytes());
                dkp::seal(&mut b, 1);
            }
        }
        r.f.store.insert(path(3, b"X"), b.clone());
        r.load(3);
        assert_eq!(r.f.toasts, [reason as u32], "{what}");
        assert_eq!(r.f.store[&path(3, b"X")], b, "{what}: left in place");
    }
}
