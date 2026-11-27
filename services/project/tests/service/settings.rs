//! `dk_project_settings`: the saved settings of a module in the open project (live image) and in another project (one read of its file).
use crate::browse::{file_full, rig_with_files};
use crate::*;
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_CHANGED, E_NOT_FOUND};
use dkrt_svc_project::engine::*;

const BINARY: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];

/// What a call gave: (status, next, the entries as (key, head, full length)).
type Answer = (i32, u32, Vec<(Vec<u8>, Vec<u8>, u16)>);

fn settings(
    r: &mut Rig,
    slot: u32,
    module: &[u8],
    version: u32,
    cursor: u32,
    max: usize,
) -> Answer {
    let mut out = vec![ProjectSetting::EMPTY; max];
    let mut next = 99;
    let status =
        r.p.settings(&r.f, slot, module, version, cursor, &mut out, &mut next);
    let n = status.max(0) as usize;
    let entries = out[..n]
        .iter()
        .map(|e| {
            (
                e.key[..e.key_len as usize].to_vec(),
                e.head[..e.head_len as usize].to_vec(),
                e.value_len,
            )
        })
        .collect();
    (status, next, entries)
}

/// Slot 7 (the open project, counter 3) and slot 20 (counter 4) both hold six entries of `a/x`, one of `b/y` and list a module `c/z` without entries.
fn rig_with_settings() -> Rig {
    let long = vec![b'x'; 100];
    let modules: &[(&[u8], u32)] = &[(b"a/x", 1), (b"b/y", 2), (b"c/z", 3)];
    let data: Vec<(&[u8], &[u8], &[u8])> = vec![
        (b"a/x", b"one", b"1"),
        (b"a/x", b"bin", BINARY),
        (b"b/y", b"other", b"o"),
        (b"a/x", b"long", &long),
        (b"a/x", b"four", b"4"),
        (b"a/x", b"five", b"5"),
        (b"a/x", b"six", b"6"),
    ];
    let mut r = rig_with_files();
    r.f.store.insert(
        path(7, b"SONG"),
        file_full(7, b"SONG", 0x77, 3, modules, &data),
    );
    r.f.store.insert(
        path(20, b"TWENTY"),
        file_full(20, b"TWENTY", 0x2020, 4, modules, &data),
    );
    r.load(7);
    r.settle();
    r
}

fn keys(v: &[(Vec<u8>, Vec<u8>, u16)]) -> Vec<&[u8]> {
    v.iter().map(|e| e.0.as_slice()).collect()
}

#[test]
fn the_open_project_answers_at_once_with_bare_keys_and_whole_short_values() {
    let mut r = rig_with_settings();
    let (status, next, e) = settings(&mut r, 7, b"a/x", 3, 0, 16);
    assert_eq!((status, next), (6, 0));
    assert_eq!(
        keys(&e),
        [&b"one"[..], b"bin", b"long", b"four", b"five", b"six"]
    );
    assert_eq!(e[1], (b"bin".to_vec(), BINARY.to_vec(), 8));
    assert!(!r.p.busy(&r.job), "no read was queued");
}

#[test]
fn a_value_over_64_bytes_gives_its_first_64_and_its_full_length() {
    let mut r = rig_with_settings();
    let (_, _, e) = settings(&mut r, 7, b"a/x", 3, 0, 16);
    assert_eq!(e[2], (b"long".to_vec(), vec![b'x'; 64], 100));
}

#[test]
fn pages_continue_with_next_and_the_last_page_says_zero() {
    let mut r = rig_with_settings();
    let (status, next, e) = settings(&mut r, 7, b"a/x", 3, 0, 4);
    assert_eq!((status, next), (4, 4));
    assert_eq!(keys(&e)[3], b"four");
    let (status, next, e) = settings(&mut r, 7, b"a/x", 3, 4, 4);
    assert_eq!((status, next), (2, 0));
    assert_eq!(keys(&e), [&b"five"[..], b"six"]);
    // a page that ends exactly at the last entry has no next
    let (status, next, _) = settings(&mut r, 7, b"a/x", 3, 2, 4);
    assert_eq!((status, next), (4, 0));
    // past the end: nothing
    assert_eq!(settings(&mut r, 7, b"a/x", 3, 6, 4).0, 0);
}

#[test]
fn another_write_counter_says_changed_and_writes_nothing() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 7, b"a/x", 2, 0, 4).0, E_CHANGED);
    // a save moves the counter: a page that asks with the old one is told
    assert_eq!(r.put(b"k", b"v"), 0);
    r.hook_save(7);
    r.settle();
    assert_eq!(settings(&mut r, 7, b"a/x", 3, 0, 4).0, E_CHANGED);
    // the counter is 4 now (the save rewrote the module list, so the module may be gone: not found, not changed)
    assert_ne!(settings(&mut r, 7, b"a/x", 4, 0, 4).0, E_CHANGED);
}

#[test]
fn a_module_the_project_does_not_list_is_not_found_and_a_listed_one_may_be_empty() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 7, b"d/w", 3, 0, 4).0, E_NOT_FOUND);
    assert_eq!(settings(&mut r, 7, b"", 3, 0, 4).0, E_NOT_FOUND);
    assert_eq!(settings(&mut r, 7, b"c/z", 3, 0, 4), (0, 0, vec![]));
    assert_eq!(settings(&mut r, 99, b"a/x", 3, 0, 4).0, E_NOT_FOUND);
}

#[test]
fn another_project_says_try_again_then_gives_a_page_from_its_file() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 20, b"a/x", 4, 0, 16).0, E_AGAIN);
    assert_eq!(
        settings(&mut r, 20, b"a/x", 4, 0, 16).0,
        E_AGAIN,
        "asking again does not answer early"
    );
    r.settle();
    let (status, next, e) = settings(&mut r, 20, b"a/x", 4, 0, 16);
    assert_eq!(
        (status, next),
        (PAGE_MAX as i32, PAGE_MAX as u32),
        "one read keeps a page of PAGE_MAX entries"
    );
    assert_eq!(keys(&e), [&b"one"[..], b"bin", b"long", b"four"]);
    assert_eq!(e[2], (b"long".to_vec(), vec![b'x'; 64], 100));
    // the next page is another read
    let reads = r.f.reads;
    let more = PAGE_MAX as u32;
    assert_eq!(settings(&mut r, 20, b"a/x", 4, more, 16).0, E_AGAIN);
    r.settle();
    assert!(r.f.reads > reads);
    let (status, next, e) = settings(&mut r, 20, b"a/x", 4, more, 16);
    assert_eq!((status, next), (2, 0));
    assert_eq!(keys(&e), [&b"five"[..], b"six"]);
    assert_eq!(r.p.slot(), 7, "reading it for the manager binds nothing");
}

#[test]
fn a_smaller_max_than_the_kept_page_continues_inside_it() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 20, b"a/x", 4, 0, 2).0, E_AGAIN);
    r.settle();
    let (status, next, e) = settings(&mut r, 20, b"a/x", 4, 0, 2);
    assert_eq!((status, next), (2, 2));
    assert_eq!(keys(&e), [&b"one"[..], b"bin"]);
}

#[test]
fn another_project_with_another_counter_says_changed_and_a_cleared_file_not_found() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 20, b"a/x", 3, 0, 4).0, E_AGAIN);
    r.settle();
    assert_eq!(settings(&mut r, 20, b"a/x", 3, 0, 4).0, E_CHANGED);
    assert_eq!(settings(&mut r, 20, b"a/x", 4, 0, 4).0, 4);
    r.hook_clear(20);
    r.settle();
    assert_eq!(settings(&mut r, 20, b"a/x", 4, 0, 4).0, E_AGAIN);
    r.settle();
    assert_eq!(settings(&mut r, 20, b"a/x", 4, 0, 4).0, E_NOT_FOUND);
}

#[test]
fn another_project_without_the_module_or_without_a_listing_is_not_found() {
    let mut r = rig_with_settings();
    assert_eq!(settings(&mut r, 20, b"d/w", 4, 0, 4).0, E_AGAIN);
    r.settle();
    assert_eq!(settings(&mut r, 20, b"d/w", 4, 0, 4).0, E_NOT_FOUND);
    // a slot the firmware does not list is not found at once, nothing is queued
    assert_eq!(settings(&mut r, 99, b"a/x", 4, 0, 4).0, E_NOT_FOUND);
    assert!(!r.p.busy(&r.job));
}
