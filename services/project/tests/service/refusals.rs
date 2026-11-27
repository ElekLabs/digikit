//! Stores that stay busy or refuse a load, a clear or an archive probe.
use crate::*;
use dkrt_sdk::logk::{PROJECT_DEFERRED, PROJECT_IGNORED};
use dkrt_sdk::E_BUSY;
use dkrt_svc_project::dkp::Ignore;
use dkrt_svc_project::engine::*;

/// Passes with the clock moving `RETRY_MS` and a little more each time, `rounds` times.
pub(crate) fn settle_over(r: &mut Rig, rounds: u32) {
    for _ in 0..rounds {
        r.f.now += RETRY_MS + 1;
        r.settle();
    }
}

#[test]
fn a_load_whose_store_stays_busy_gives_up_after_30_seconds_and_leaves_no_project_bound() {
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(5, b"NEW", 2);
    r.f.store.insert(
        path(4, b"OLD"),
        file_bytes(4, b"OLD", 1, 1, &[(b"test/owner/k", b"old")]),
    );
    r.load(4);
    assert_eq!(r.p.slot(), 4);
    r.f.refuse_list = u32::MAX;
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), 4, "while it retries the old project stays");
    settle_over(&mut r, 29);
    assert_eq!(r.p.slot(), 4, "29 seconds are not 30");
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    settle_over(&mut r, 3);
    assert_eq!(r.p.slot(), -1, "no project: module writes go to the record");
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), dkrt_sdk::proto::E_SET_OFF);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "one event");
    assert_eq!(r.f.last(PROJECT_DEFERRED)[..2], [5, E_BUSY as u32]);
    assert_eq!(r.f.toasts, [Ignore::Store as u32], "one warning");
    // it does not ask again
    let left = r.f.refuse_list;
    settle_over(&mut r, 5);
    assert_eq!(r.f.refuse_list, left);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
}

#[test]
fn a_load_the_store_refuses_for_another_reason_ends_at_once_unbound_with_one_event_and_one_warning()
{
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(5, b"NEW", 2);
    r.load(4);
    r.f.refuse_list = 2;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0, "an answer is no deferral");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    assert_eq!(
        r.f.last(PROJECT_IGNORED)[..3],
        [5, Ignore::Store as u32, dkrt_sdk::proto::E_NOSTORE as u32]
    );
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
    assert_eq!(r.p.status() & (ST_DEFERRED | ST_IGNORED), 0);
    settle_over(&mut r, 3);
    assert_eq!(r.f.refuse_list, 1, "no retry");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
}

#[test]
fn a_clear_the_store_refuses_for_another_reason_is_logged_and_dropped() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = 2;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 3);
    assert_eq!(r.f.refuse_list, 1, "no retry");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    assert!(r.f.toasts.is_empty(), "only a load warns");
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
}

#[test]
fn an_archive_probe_that_meets_a_busy_store_waits_and_never_takes_the_path() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.busy_reads.insert(archive(b"PROJ", 0x55), 1);
    r.hook_clear(5);
    for _ in 0..5 {
        r.pass();
    }
    assert!(
        r.f.ops.is_empty() && archived(&r).is_empty(),
        "nothing yet: the probe waits"
    );
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(
        archived(&r),
        [archive(b"PROJ", 0x55)],
        "the plain path, not a suffix"
    );
    assert_eq!(r.f.store[&archive(b"PROJ", 0x55)], file);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}

#[test]
fn a_file_that_is_archived_already_is_only_removed_from_its_slot() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    // a cut after the archive copy was written and before the source was removed
    r.f.store.insert(archive(b"PROJ", 0x55), file.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(archived(&r), [archive(b"PROJ", 0x55)], "no second copy");
    assert_eq!(r.f.store[&archive(b"PROJ", 0x55)], file);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
    assert_eq!(
        r.f.ops,
        [('d', path(5, b"PROJ"))],
        "one removal and no write"
    );
}
