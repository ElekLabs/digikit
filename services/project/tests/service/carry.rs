//! The state a reload carries, over the fake: bytes that are refused are told (one log line, one toast) and leave a project that is named and unbound; a deadline is carried as
//! the time left and counts from the new copy's clock.
use crate::*;
use dkrt_sdk::logk::PROJECT_IGNORED;
use dkrt_svc_project::dkp::Ignore;
use dkrt_svc_project::engine::*;

const DROPPED: u32 = Ignore::CarryDropped as u32;

fn export(r: &Rig) -> Vec<u8> {
    let mut out = vec![0u8; CARRY_MAX];
    let n = r.p.export_state(&r.f, r.f.now, &r.pend, &mut out);
    out.truncate(n);
    out
}

/// A new copy on the same fake, whose clock restarted at `now`.
fn new_copy(r: &mut Rig, now: u32, bytes: Option<&[u8]>) -> ProjectService {
    let mut p = ProjectService::new();
    r.f.now = now;
    p.import_state(&mut r.f, &r.pend, bytes);
    p
}

fn bound_5() -> (Rig, Vec<u8>) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]),
    );
    r.load(5);
    assert_eq!(r.p.slot(), 5);
    let bytes = export(&r);
    (r, bytes)
}

#[test]
fn a_round_trip_of_a_bound_project_is_silent() {
    let (mut r, bytes) = bound_5();
    let p = new_copy(&mut r, 1000, Some(&bytes));
    assert_eq!(p.slot(), 5);
    assert!(r.f.toasts.is_empty() && r.f.count(PROJECT_IGNORED) == 0);
}

#[test]
fn a_first_start_is_unbound_and_silent_and_dropped_state_is_one_toast_and_one_log_line() {
    let mut r = Rig::new();
    let p = new_copy(&mut r, 1000, None);
    assert_eq!(p.slot(), -1);
    assert!(r.f.toasts.is_empty() && r.f.count(PROJECT_IGNORED) == 0);
    for bytes in [&[1u8, 2, 3][..], &[][..]] {
        let mut r = Rig::new();
        let p = new_copy(&mut r, 1000, Some(bytes));
        assert_eq!(p.slot(), -1);
        assert_eq!(r.f.toasts, [DROPPED]);
        assert_eq!(r.f.count(PROJECT_IGNORED), 1);
        assert_eq!(r.f.last(PROJECT_IGNORED), [0x80, DROPPED, 0, 0, 0]);
    }
}

// Offsets in the carried bytes: tag 0, slot 1..5, deadline 5..9, live stamp 9..14, has_file 14, device 15..17, file slot 17, name length 18, path length 47, path 48..
#[test]
fn a_field_that_a_file_check_refuses_drops_the_state_without_a_panic() {
    let (mut r, bytes) = bound_5();
    assert_eq!(bytes[14], 1, "the project has a file");
    let image = bytes.windows(4).position(|w| w == b"DKPJ").unwrap();
    let cases: [(&str, Vec<(usize, u8)>); 9] = [
        ("name length 17", vec![(18, 17)]),
        ("name length 255", vec![(18, 255)]),
        ("file slot 200", vec![(17, 200)]),
        ("bound slot 200", vec![(4, 200)]),
        ("another device", vec![(16, 0x99)]),
        ("path byte below 0x20", vec![(47, 1), (48, 1)]),
        ("path byte above 0x7e", vec![(47, 1), (48, 0xff)]),
        ("path length over the bytes", vec![(47, 255)]),
        ("image that is no image", vec![(image, 0)]),
    ];
    for (what, edits) in cases {
        let mut b = bytes.clone();
        for (at, v) in edits {
            b[at] = v;
        }
        r.f.toasts.clear();
        let p = new_copy(&mut r, 1000, Some(&b));
        assert_eq!(p.slot(), -1, "{what}");
        assert_eq!(r.f.toasts, [DROPPED], "{what}");
    }
}

#[test]
fn bytes_changed_or_cut_anywhere_never_panic() {
    let (mut r, bytes) = bound_5();
    for at in (0..bytes.len()).step_by(3) {
        for v in [0u8, 0x7f, 0xff] {
            let mut b = bytes.clone();
            b[at] = v;
            let _ = new_copy(&mut r, 1000, Some(&b));
        }
    }
    for len in (0..bytes.len()).step_by(97) {
        let p = new_copy(&mut r, 1000, Some(&bytes[..len]));
        assert_eq!(p.slot(), -1, "cut at {len}");
    }
}

#[test]
fn the_retry_of_a_failed_load_counts_from_the_new_copys_clock() {
    let (mut r, _) = failed_load_of_5();
    let bytes = export(&r);
    let mut p = Box::new(new_copy(&mut r, 5, Some(&bytes)));
    assert!(
        r.f.toasts.len() == 1 && r.f.count(PROJECT_IGNORED) == 1,
        "only the refused load"
    );
    // the old deadline (31000) is far past the new clock (5): the retry waits for the 30 s that were left
    r.f.refuse_list = 1;
    r.f.now = 5 + 29_000;
    p.tick(&mut r.f, &r.pend, &r.job);
    assert_eq!(r.f.refuse_list, 1, "29 seconds: not asked");
    r.f.now = 5 + 30_000;
    for _ in 0..3 {
        p.tick(&mut r.f, &r.pend, &r.job);
    }
    assert_eq!(r.f.refuse_list, 0, "30 seconds: the store was asked");
}

#[test]
fn the_wait_of_the_restore_counts_from_the_new_copys_clock() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.playing = true;
    r.hook_slot(7);
    for _ in 0..6 {
        r.pass();
    }
    let bytes = export(&r);
    r.f.playing = false;
    r.pend.put_back(&r.f, &Pending::new());
    let mut p = Box::new(new_copy(&mut r, 3, Some(&bytes)));
    let asked = r.f.live_loads.len();
    for _ in 0..6 {
        p.tick(&mut r.f, &r.pend, &r.job);
    }
    assert_eq!(
        r.f.live_loads.len(),
        asked,
        "the wait is not over at the new clock"
    );
    r.f.now = 3 + PLAY_WAIT_MS + 1;
    for _ in 0..6 {
        p.tick(&mut r.f, &r.pend, &r.job);
    }
    assert!(
        r.f.live_loads.len() > asked,
        "asked again when the time left is over"
    );
}
