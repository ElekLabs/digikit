//! `dk_project_read`: the raw bytes of a project, the open one too, from one read of its file on the card.
use crate::browse::{file_full, rig_with_files};
use crate::*;
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_CHANGED, E_NOT_FOUND};
use dkrt_svc_project::engine::*;

fn read(r: &mut Rig, slot: u32, version: u32, offset: u32, max: usize) -> (i32, u32, Vec<u8>) {
    let mut out = vec![0xEE; max];
    let mut total = 99;
    let status = r.p.read(&r.f, slot, version, offset, &mut out, &mut total);
    out.truncate(status.max(0) as usize);
    (status, total, out)
}

/// Slots 7 (open, counter 3) and 20 (counter 4) hold the same module and `n` values of 200 bytes: 8 make a file that one kept window holds whole, 25 one that is longer.
fn rig_with(n: usize) -> (Rig, Vec<u8>, Vec<u8>) {
    let modules: &[(&[u8], u32)] = &[(b"a/x", 1)];
    let values: Vec<Vec<u8>> = (0..n).map(|i| vec![i as u8; 200]).collect();
    let keys: Vec<Vec<u8>> = (0..n).map(|i| format!("k{i:02}").into_bytes()).collect();
    let data: Vec<(&[u8], &[u8], &[u8])> = (0..n)
        .map(|i| (&b"a/x"[..], keys[i].as_slice(), values[i].as_slice()))
        .collect();
    let open = file_full(7, b"SONG", 0x77, 3, modules, &data);
    let other = file_full(20, b"TWENTY", 0x2020, 4, modules, &data);
    let mut r = rig_with_files();
    r.f.store.insert(path(7, b"SONG"), open.clone());
    r.f.store.insert(path(20, b"TWENTY"), other.clone());
    r.load(7);
    r.settle();
    (r, open, other)
}

fn rig_with_raw() -> (Rig, Vec<u8>, Vec<u8>) {
    rig_with(25)
}

#[test]
fn the_open_project_is_read_from_its_file_and_pages_inside_the_window_read_nothing_again() {
    let (mut r, open, _) = rig_with(8);
    assert!(open.len() < RAW_MAX, "the file is held whole");
    assert_eq!(
        read(&mut r, 7, 3, 0, 100).0,
        E_AGAIN,
        "read from the card, not answered live"
    );
    r.settle();
    let reads = r.f.reads;
    let (status, total, bytes) = read(&mut r, 7, 3, 0, 100);
    assert_eq!((status, total), (100, open.len() as u32));
    assert_eq!(bytes, open[..100]);
    let (status, _, bytes) = read(&mut r, 7, 3, 1000, 4000);
    assert_eq!(
        status as usize,
        open.len() - 1000,
        "the last range is short"
    );
    assert_eq!(bytes, open[1000..]);
    assert_eq!(
        read(&mut r, 7, 3, open.len() as u32, 10).0,
        0,
        "at the end: nothing"
    );
    assert_eq!(read(&mut r, 7, 3, 99999, 10).0, 0);
    assert_eq!(r.f.reads, reads, "paging did not read the card again");
    assert_eq!(r.p.slot(), 7, "reading it binds nothing");
}

#[test]
fn the_settings_and_the_raw_bytes_of_one_project_share_a_read() {
    let (mut r, _, other) = rig_with(8);
    let mut page = [ProjectSetting::EMPTY; 4];
    let mut next = 0;
    let mut ask_settings = |r: &mut Rig| r.p.settings(&r.f, 20, b"a/x", 4, 0, &mut page, &mut next);
    assert_eq!(ask_settings(&mut r), E_AGAIN);
    r.settle();
    assert_eq!(ask_settings(&mut r), 4);
    // the first raw request reads the file once more and keeps the page too
    assert_eq!(read(&mut r, 20, 4, 0, 10).0, E_AGAIN);
    r.settle();
    let reads = r.f.reads;
    assert_eq!(read(&mut r, 20, 4, 0, 10).2, other[..10]);
    assert_eq!(ask_settings(&mut r), 4, "the page is still held");
    assert_eq!(read(&mut r, 20, 4, 500, 10).2, other[500..510]);
    assert_eq!(
        r.f.reads, reads,
        "neither kind of request read the card again"
    );
}

#[test]
fn a_bad_counter_says_changed_and_an_unlisted_slot_not_found() {
    let (mut r, _, _) = rig_with_raw();
    assert_eq!(read(&mut r, 7, 2, 0, 10).0, E_AGAIN);
    r.settle();
    assert_eq!(read(&mut r, 7, 2, 0, 10), (E_CHANGED, 0, vec![]));
    assert_eq!(read(&mut r, 99, 3, 0, 10).0, E_NOT_FOUND);
}

#[test]
fn another_project_says_try_again_then_gives_a_range_of_its_file() {
    let (mut r, _, other) = rig_with_raw();
    assert_eq!(read(&mut r, 20, 4, 0, 8000).0, E_AGAIN);
    assert_eq!(
        read(&mut r, 20, 4, 0, 8000).0,
        E_AGAIN,
        "asking again does not answer early"
    );
    r.settle();
    let (status, total, bytes) = read(&mut r, 20, 4, 0, 8000);
    assert_eq!(
        (status, total),
        (RAW_MAX as i32, other.len() as u32),
        "one read keeps a window of RAW_MAX bytes"
    );
    assert_eq!(bytes, other[..RAW_MAX]);
    // a range inside the window needs no read; the next window is another read
    let reads = r.f.reads;
    assert_eq!(read(&mut r, 20, 4, 100, 8000).2, other[100..RAW_MAX]);
    assert_eq!(r.f.reads, reads);
    assert_eq!(read(&mut r, 20, 4, RAW_MAX as u32, 8000).0, E_AGAIN);
    r.settle();
    assert!(r.f.reads > reads);
    let (status, total, bytes) = read(&mut r, 20, 4, RAW_MAX as u32, 8000);
    assert_eq!(total, other.len() as u32);
    assert_eq!(bytes, other[RAW_MAX..RAW_MAX + status as usize]);
    assert_eq!(r.p.slot(), 7, "reading it binds nothing");
}

#[test]
fn a_smaller_max_than_the_kept_range_continues_inside_it() {
    let (mut r, _, other) = rig_with_raw();
    assert_eq!(read(&mut r, 20, 4, 0, 10).0, E_AGAIN);
    r.settle();
    let (status, _, bytes) = read(&mut r, 20, 4, 0, 10);
    assert_eq!(status, 10);
    assert_eq!(bytes, other[..10]);
}

#[test]
fn another_project_with_another_counter_says_changed_and_a_cleared_file_not_found() {
    let (mut r, _, _) = rig_with_raw();
    assert_eq!(read(&mut r, 20, 3, 0, 10).0, E_AGAIN);
    r.settle();
    assert_eq!(read(&mut r, 20, 3, 0, 10).0, E_CHANGED);
    assert_eq!(read(&mut r, 20, 4, 0, 10).0, 10);
    r.hook_clear(20);
    r.settle();
    assert_eq!(read(&mut r, 20, 4, 0, 10).0, E_AGAIN);
    r.settle();
    assert_eq!(read(&mut r, 20, 4, 0, 10).0, E_NOT_FOUND);
}

#[test]
fn a_range_past_the_end_of_another_file_is_empty_with_the_total() {
    let (mut r, _, other) = rig_with_raw();
    let at = other.len() as u32 + 5;
    assert_eq!(read(&mut r, 20, 4, at, 10).0, E_AGAIN);
    r.settle();
    assert_eq!(read(&mut r, 20, 4, at, 10), (0, other.len() as u32, vec![]));
}
