//! The write job: a busy store, a cut write, the mirror slot, waits, status, deferrals.
use crate::*;
use dkrt_sdk::logk::{PROJECT_DEFERRED, PROJECT_IGNORED, PROJECT_SAVED};
use dkrt_sdk::proto::E_SET_NOKEY;
use dkrt_sdk::E_BUSY;
use dkrt_svc_project::dkp::{self, Ignore};
use dkrt_svc_project::engine::*;

#[test]
fn a_busy_store_defers_after_30_seconds_keeps_the_data_dirty_and_the_next_save_writes() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"v");
    r.f.busy = true;
    r.hook_save(12);
    // each pass the job answers E_BUSY 1000 times, fails, and the tick tries again a second later
    let mut deferred_at = None;
    for _ in 0..100 {
        r.pass();
        if r.f.count(PROJECT_DEFERRED) > 0 && deferred_at.is_none() {
            deferred_at = Some(r.f.now);
        }
        r.f.now += 500;
        if r.f.count(PROJECT_SAVED) > 0 {
            break;
        }
    }
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "once");
    let waited = deferred_at.unwrap() - 1000;
    assert!((30_000..=32_000).contains(&waited), "{waited}");
    assert_eq!(r.f.last(PROJECT_DEFERRED)[1] as i32, E_BUSY);
    assert_eq!(
        r.f.last(PROJECT_SAVED)[3] as i32,
        E_BUSY,
        "SAVED carries the code"
    );
    assert_eq!(
        r.p.status() & (ST_DIRTY | ST_DEFERRED),
        ST_DIRTY | ST_DEFERRED
    );
    // no retry loop after the event: more passes do not touch the store
    for _ in 0..10 {
        r.pass();
        r.f.now += 1000;
    }
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    // the store is free: the next save writes
    r.f.busy = false;
    r.hook_save(12);
    r.settle();
    assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_busy_store_that_frees_within_a_second_writes_without_an_event() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"v");
    r.f.busy = true;
    r.hook_save(12);
    r.pass();
    assert!(r.f.store.is_empty());
    r.f.busy = false;
    r.f.now += 1000;
    r.settle();
    assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    assert_eq!(r.f.last(PROJECT_SAVED)[3], 0);
}

#[test]
fn a_cut_in_the_middle_of_a_write_keeps_the_old_file() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    let old = file_bytes(12, b"MY SONG", 5, 1, &[(b"test/owner/k", b"old")]);
    r.f.store.insert(path(12, b"MY SONG"), old.clone());
    r.load(12);
    r.put(b"k", b"new");
    r.f.cut_before_close = true;
    r.hook_save(12);
    for _ in 0..5 {
        r.pass();
    }
    // the file session is open and never closed: the power is gone
    assert!(r.f.session.is_some());
    assert_eq!(r.f.store[&path(12, b"MY SONG")], old);
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
}

#[test]
fn a_save_as_keeps_the_data_and_a_new_project_does_not() {
    // save-as: the setter and the save name slot 30, the firmware's live stamp is the one the image was bound to
    let mut r = Rig::new();
    r.rec(7, b"WORK", 0x10);
    r.load(7);
    r.put(b"k", b"mine");
    r.rec(30, b"WORK", 0x10);
    r.hook_slot(30);
    r.hook_save(30);
    r.settle();
    assert_eq!(r.p.slot(), 30);
    assert_eq!(
        dkp::check(&r.f.store[&path(30, b"WORK")], DEV)
            .unwrap()
            .slot,
        30
    );
    assert_eq!(r.get(b"k"), b"mine");
    // a new project: the live stamp changed (the firmware made a new one), so the data does not follow
    let mut r = Rig::new();
    r.rec(7, b"WORK", 0x10);
    r.load(7);
    r.put(b"k", b"mine");
    r.f.recs.insert(
        0x80,
        SlotRecord {
            name: n16(b"live"),
            stamp: 0xbbbb_0002,
        },
    );
    r.rec(31, b"NEW PROJECT", 0x20);
    r.hook_slot(31);
    r.hook_save(31);
    r.settle();
    assert_eq!(r.p.slot(), 31);
    assert!(r.f.store.is_empty(), "no data, no file");
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
}

#[test]
fn the_mirror_slot_and_a_slot_past_127_never_write() {
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    for s in [0x80u32, 200, u32::MAX] {
        r.hook_save(s);
        r.settle();
    }
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
}

#[test]
fn a_load_waits_while_a_live_operation_runs() {
    let mut r = Rig::new();
    r.rec(12, b"A", 1);
    r.f.store.insert(
        path(12, b"A"),
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"v")]),
    );
    r.f.live_op = 3;
    r.hook_slot(12);
    for _ in 0..5 {
        r.pass();
    }
    assert_eq!(r.f.reads, 0);
    assert_eq!(r.p.slot(), -1);
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_save_waits_for_the_firmwares_record_of_its_slot_and_drops_after_two_seconds() {
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    r.hook_save(7);
    r.f.recs.remove(&7);
    r.pass();
    assert!(r.f.store.is_empty());
    r.f.recs.insert(
        7,
        SlotRecord {
            name: n16(b"WORK"),
            stamp: 1,
        },
    );
    r.settle();
    assert!(r.f.store.contains_key(&path(7, b"WORK")));
    // no record for longer than the wait: the save is dropped
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    r.f.recs.remove(&7);
    r.hook_save(7);
    r.pass();
    r.f.now += REC_WAIT_MS + 1;
    r.settle();
    assert!(r.f.store.is_empty());
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [7, Ignore::SaveNoRecord as u32, 0, 0, 0],
        "the dropped save is logged"
    );
}

#[test]
fn the_hooks_record_is_taken_with_interrupts_masked_and_cleared() {
    let mut r = Rig::new();
    r.rec(4, b"X", 1);
    r.hook_slot(4);
    r.pass();
    assert_eq!(r.f.irq.get(), 0, "masks are balanced");
    assert_eq!(r.pend.slot_flag, 0);
    assert_eq!(r.pend.clear, [0; 16]);
}

#[test]
fn any_status_but_busy_ends_the_attempt_with_no_retry_and_the_data_stays_dirty() {
    for code in [
        dkrt_sdk::proto::E_FULL,
        dkrt_sdk::proto::E_INUSE,
        dkrt_sdk::proto::E_NOSTORE,
        dkrt_sdk::proto::E_WRITE,
    ] {
        let mut r = Rig::new();
        r.rec(12, b"MY SONG", 5);
        r.load(12);
        r.put(b"k", b"v");
        r.f.fail_create = Some(code);
        r.hook_save(12);
        r.settle();
        assert_eq!(r.f.creates, 1, "{code}: one try");
        assert_eq!(r.f.last(PROJECT_SAVED)[3] as i32, code);
        assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
        for _ in 0..5 {
            r.f.now += 2000;
            r.pass();
        }
        assert_eq!(r.f.creates, 1, "{code}: no retry loop");
        assert_eq!(r.p.status(), ST_DIRTY);
        // the next save writes
        r.f.fail_create = None;
        r.hook_save(12);
        r.settle();
        assert_eq!(r.p.status(), ST_FILE);
        assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    }
}

#[test]
fn a_put_after_the_freeze_keeps_the_image_dirty_for_the_next_save() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"one");
    r.hook_save(12);
    // up to the post of the job (list, probe), then a put before the job ends
    for _ in 0..4 {
        r.p.tick(&mut r.f, &r.pend, &r.job);
        if r.f.posted {
            break;
        }
    }
    assert!(r.f.posted);
    r.put(b"k", b"two");
    r.pump();
    r.settle();
    assert_eq!(r.p.status(), ST_FILE | ST_DIRTY);
    r.hook_save(12);
    r.settle();
    assert_eq!(r.p.status(), ST_FILE);
    assert_eq!(dkp::head_of(&r.f.store[&path(12, b"MY SONG")]).counter, 2);
}

#[test]
fn one_deferral_is_logged_once_by_every_kind_of_work_until_a_write_succeeds() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.rec(6, b"NEXT", 0x66);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = u32::MAX;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    assert_eq!(r.p.status() & ST_DEFERRED, ST_DEFERRED);
    // a load that gives up in the same deferral logs nothing more, but it warns
    r.hook_slot(6);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "once");
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
}

#[test]
fn an_answer_that_is_no_busy_ends_the_busy_streak() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = u32::MAX;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 25);
    // the store answers, with a refusal that is no busy answer
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    settle_over(&mut r, 1);
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    // busy again: the 30 seconds start over from here, not from the first busy answer
    r.f.refuse_status = E_BUSY;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 20);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0, "20 seconds are not 30");
    settle_over(&mut r, 12);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
}
