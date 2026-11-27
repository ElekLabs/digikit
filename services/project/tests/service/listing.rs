//! Listings the store refuses or ends half way.
use crate::*;
use dkrt_sdk::logk::PROJECT_LOADED;
use dkrt_svc_project::engine::*;

#[test]
fn a_clear_whose_listing_the_store_refuses_runs_again_after_the_retry_time() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = 1;
    r.hook_clear(5);
    r.settle();
    assert!(r.f.ops.is_empty(), "nothing yet: the clear waits");
    r.settle();
    assert!(
        r.f.ops.is_empty(),
        "and it waits for the clock, not for the passes"
    );
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(archived(&r), [archive(b"PROJ", 0x55)]);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}

#[test]
fn a_load_whose_listing_fails_half_way_does_not_bind_from_partial_rows() {
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
    r.f.fail_poll_once = true;
    r.load(12);
    assert_eq!(r.p.slot(), -1, "the project is not bound with no file");
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(r.p.slot(), 12);
    assert_eq!(r.get(b"pool"), b"abc");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_save_that_archives_the_refused_file_ends_the_ignored_state() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    let stale = file_bytes(
        12,
        b"MY SONG",
        0x0bad_f00d,
        1,
        &[(b"test/owner/pool", b"old")],
    );
    r.f.store.insert(path(12, b"MY SONG"), stale);
    r.load(12);
    assert_eq!(r.p.status(), ST_IGNORED);
    r.put(b"k", b"mine");
    r.hook_save(12);
    r.settle();
    assert_eq!(archived(&r), [archive(b"MY SONG", 0x0bad_f00d)]);
    assert_eq!(
        r.p.status(),
        ST_FILE,
        "written, clean and no longer ignored"
    );
}
