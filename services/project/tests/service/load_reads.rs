//! The second read of a load.
use crate::*;
use dkrt_sdk::logk::{PROJECT_IGNORED, PROJECT_LOADED};
use dkrt_sdk::{E_BUSY, E_NOT_FOUND};
use dkrt_svc_project::dkp::Ignore;

/// A rig whose project 5 has a file; the listing finds it and then the read of the load answers as `script` says (the listing's own read comes first).
pub(crate) fn load_of_5_with_reads(script: &[Option<i32>]) -> (Rig, Vec<u8>) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.read_script
        .insert(path(5, b"PROJ"), script.iter().copied().collect());
    r.load(5);
    (r, file)
}

#[test]
fn a_refused_second_read_of_a_load_fails_it_and_a_later_save_keeps_the_file() {
    let status = dkrt_sdk::proto::E_NOSTORE;
    let (mut r, file) = load_of_5_with_reads(&[None, Some(status)]);
    assert_eq!(r.p.slot(), -1, "no empty project is bound");
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [5, Ignore::Store as u32, status as u32, 0, 0]
    );
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    r.hook_save(5);
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty() && r.f.ops.is_empty());
}

#[test]
fn a_busy_second_read_of_a_load_is_tried_again_like_a_busy_listing() {
    let (mut r, _) = load_of_5_with_reads(&[None, Some(E_BUSY)]);
    assert_eq!(r.p.slot(), -1, "waits for the retry time");
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
}

#[test]
fn a_file_that_is_gone_at_the_second_read_binds_the_slot_empty() {
    let (r, _) = load_of_5_with_reads(&[None, Some(E_NOT_FOUND)]);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(&r.f.last(PROJECT_LOADED)[..4], &[5, 0, 0, 0]);
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
    assert!(r.f.toasts.is_empty());
}

#[test]
fn a_failed_load_is_tried_again_every_30_seconds_without_new_events() {
    let (mut r, _) = failed_load_of_5();
    // the retry is refused again: nothing new is logged or shown
    r.f.refuse_list = 1;
    settle_over(&mut r, 29);
    assert_eq!(r.p.slot(), -1, "29 seconds are not 30");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), -1, "the retry ran and was refused");
    assert_eq!(r.f.refuse_list, 0, "it asked the store");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1, "no second IGNORED");
    assert_eq!(r.f.toasts.len(), 1, "no second toast");
    // a live operation holds the retry back
    r.f.live_op = 1;
    settle_over(&mut r, 31);
    assert_eq!(r.p.slot(), -1);
    r.f.live_op = 0;
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
}
