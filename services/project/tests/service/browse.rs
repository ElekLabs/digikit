//! What the Project Manager reads: `list` (the firmware's projects with the data bit) and `get` (the summary of the current project and of another slot).
use crate::*;
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_NOT_FOUND, OK};
use dkrt_svc_project::dkp::{self, Image};
use dkrt_svc_project::engine::*;

/// The entries of one `list` call as (slot, flags).
fn list(r: &Rig, first: u32, max: usize) -> Vec<(u8, u8)> {
    let mut out = vec![ProjectEntry::default(); max];
    let n = r.p.list(&r.f, first, &mut out);
    out[..n].iter().map(|e| (e.slot, e.flags)).collect()
}

fn flags_of(r: &Rig, slot: u8) -> u8 {
    list(r, 0, 128)
        .into_iter()
        .find(|e| e.0 == slot)
        .unwrap_or_else(|| panic!("slot {slot} is not listed"))
        .1
}

/// A project file with a module list and entries of the owners (owner, bare key, value).
fn file_full(
    slot: u8,
    name: &[u8],
    stamp: u32,
    counter: u32,
    modules: &[(&[u8], u32)],
    data: &[(&[u8], &[u8], &[u8])],
) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    dkp::set_modules(&mut i.0, modules).unwrap();
    for (o, k, v) in data {
        dkp::put(&mut i.0, o, k, v).unwrap();
    }
    dkp::seal(&mut i.0, counter);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

fn summary(r: &mut Rig, slot: u32) -> (i32, Box<ProjectSummary>) {
    let mut out = Box::new(ProjectSummary::new());
    let status = r.p.get(&r.f, slot, &mut out);
    (status, out)
}

fn ids(s: &ProjectSummary) -> Vec<(Vec<u8>, u32, u8, u8)> {
    s.modules[..s.module_count as usize]
        .iter()
        .map(|m| {
            (
                m.id[..m.id_len as usize].to_vec(),
                m.hash,
                m.settings,
                m.state,
            )
        })
        .collect()
}

/// A rig with the files of the project manager's cases: slot 20 (two modules, three settings of `a/x`) and slot 22 (no module).
fn rig_with_files() -> Rig {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.rec(20, b"TWENTY", 0x2020);
    r.rec(22, b"OTHER", 0x2222);
    r.f.store.insert(
        path(20, b"TWENTY"),
        file_full(
            20,
            b"TWENTY",
            0x2020,
            4,
            &[(b"a/x", 0xa1), (b"b/y", 0xb2)],
            &[
                (b"a/x", b"k1", b"1"),
                (b"a/x", b"k2", b"2"),
                (b"b/y", b"k", b"3"),
            ],
        ),
    );
    r.f.store.insert(
        path(22, b"OTHER"),
        file_full(22, b"OTHER", 0x2222, 9, &[], &[(b"c/z", b"k", b"v")]),
    );
    r
}

#[test]
fn the_list_gives_the_firmware_projects_with_the_current_flag_and_pages() {
    let mut r = Rig::new();
    r.rec(3, b"ALPHA", 1);
    r.rec(5, b"BETA", 2);
    r.rec(9, b"GAMMA", 3);
    r.load(5);
    let slots = |v: Vec<(u8, u8)>| v.iter().map(|e| e.0).collect::<Vec<_>>();
    // empty slots are skipped; only the current project has the flag
    assert_eq!(slots(list(&r, 0, 16)), [3, 5, 9]);
    assert_eq!(
        list(&r, 0, 16)
            .iter()
            .map(|e| e.1 & ENTRY_CURRENT)
            .collect::<Vec<_>>(),
        [0, ENTRY_CURRENT, 0]
    );
    // pages: from a slot, at most `max`
    assert_eq!(slots(list(&r, 0, 2)), [3, 5]);
    assert_eq!(slots(list(&r, 4, 1)), [5]);
    assert_eq!(slots(list(&r, 6, 8)), [9]);
    assert!(list(&r, 10, 8).is_empty());
    assert!(list(&r, 128, 8).is_empty());
    // the name is the firmware's, trimmed to its length
    let mut out = [ProjectEntry::default(); 1];
    assert_eq!(r.p.list(&r.f, 9, &mut out), 1);
    assert_eq!(&out[0].name[..out[0].name_len as usize], b"GAMMA");
    // the list reads no store file and posts nothing
    let reads = r.f.reads;
    list(&r, 0, 16);
    assert_eq!(r.f.reads, reads);
}

#[test]
fn the_data_bit_is_unknown_until_the_first_listing_of_the_folder_ended() {
    let mut r = rig_with_files();
    assert!(list(&r, 0, 16).iter().all(|e| e.1 & ENTRY_UNKNOWN != 0));
    // the listing is started in the first pass and polled in the next: it has not ended yet
    r.pass();
    assert!(list(&r, 0, 16).iter().all(|e| e.1 & ENTRY_UNKNOWN != 0));
    r.settle();
    assert_eq!(
        list(&r, 0, 16),
        [(7, 0), (20, ENTRY_DATA), (22, ENTRY_DATA)],
        "one bit per slot that has a file; no slot is unknown any more"
    );
}

#[test]
fn the_listing_for_the_data_bit_runs_once_and_reads_no_file() {
    let mut r = rig_with_files();
    r.settle();
    assert_eq!(r.f.reads, 0, "only the listing, no file was read");
    r.f.refuse_list = 100;
    r.settle();
    r.settle();
    assert_eq!(r.f.refuse_list, 100, "the listing is not asked for again");
}

#[test]
fn a_listing_the_store_refuses_is_asked_again_later_and_holds_no_work_back() {
    let mut r = rig_with_files();
    r.f.refuse_list = 1;
    r.settle();
    assert!(list(&r, 0, 16).iter().all(|e| e.1 & ENTRY_UNKNOWN != 0));
    assert_eq!(
        r.f.count(dkrt_sdk::logk::PROJECT_IGNORED),
        0,
        "no project's failure"
    );
    // the work of a project does not wait for it
    r.load(7);
    assert_eq!(r.p.slot(), 7);
    r.f.now += INDEX_RETRY_MS + 1;
    r.settle();
    assert_eq!(flags_of(&r, 20), ENTRY_DATA);
}

#[test]
fn a_save_a_clear_and_a_paste_keep_the_data_bit() {
    let mut r = rig_with_files();
    r.rec(9, b"NINE", 0x99);
    r.settle();
    assert_eq!(flags_of(&r, 9), 0);
    // a save of a project with data: it has a file now
    r.load(9);
    assert_eq!(r.put(b"k", b"v"), OK);
    r.hook_save(9);
    r.settle();
    assert_eq!(flags_of(&r, 9), ENTRY_CURRENT | ENTRY_DATA);
    // a clear archives the file: no data
    r.hook_clear(20);
    r.settle();
    assert_eq!(flags_of(&r, 20), 0);
    // a paste copies the file of another slot (same name and stamp)
    r.rec(50, b"OTHER", 0x2222);
    r.hook_paste(50);
    r.settle();
    assert_eq!(flags_of(&r, 50), ENTRY_DATA);
    assert_eq!(flags_of(&r, 22), ENTRY_DATA, "the source stays");
}

#[test]
fn a_file_the_loader_does_not_use_loses_the_data_bit() {
    let mut r = Rig::new();
    // the file of slot 12 has the stamp of another project
    r.rec(12, b"SONG", 0x1212);
    r.f.store.insert(
        path(12, b"SONG"),
        file_bytes(12, b"SONG", 0x9999, 1, &[(b"test/owner/k", b"v")]),
    );
    r.settle();
    assert_eq!(flags_of(&r, 12), ENTRY_DATA, "the listing sees a file");
    r.load(12);
    assert_eq!(flags_of(&r, 12), ENTRY_CURRENT, "the loader ignores it");
}

#[test]
fn get_of_the_current_project_answers_at_once_from_the_live_image() {
    let mut r = rig_with_files();
    r.f.store.insert(
        path(7, b"SONG"),
        file_full(
            7,
            b"SONG",
            0x77,
            3,
            &[(b"a/x", 0xa1), (b"b/y", 0xb2)],
            &[(b"a/x", b"k1", b"1"), (b"a/x", b"k2", b"2")],
        ),
    );
    r.load(7);
    // a/x is loaded, b/y is not
    r.f.loaded
        .push((b"a/x".to_vec(), b"modules/machine/a-x.dkm".to_vec()));
    let (status, s) = summary(&mut r, 7);
    assert_eq!(status, OK);
    assert_eq!((s.slot, s.stamp, s.counter), (7, 0x77, 3));
    assert_eq!(&s.name[..s.name_len as usize], b"SONG");
    assert_eq!(
        ids(&s),
        [
            (b"a/x".to_vec(), 0xa1, 2, MODULE_LOADED),
            (b"b/y".to_vec(), 0xb2, 0, MODULE_UNKNOWN)
        ]
    );
    // unsaved changes count: the live image is what the project holds now
    assert_eq!(r.put(b"k", b"v"), OK);
    r.p.scope_put(b"b/y", b"k", b"v");
    assert_eq!(ids(&summary(&mut r, 7).1)[1].2, 1);
    // no request was queued
    assert!(!r.p.busy(&r.job));
}

#[test]
fn get_of_another_slot_says_try_again_then_gives_the_summary_from_its_file() {
    let mut r = rig_with_files();
    r.load(7);
    r.f.loaded
        .push((b"a/x".to_vec(), b"modules/machine/a-x.dkm".to_vec()));
    let loaded_events = r.f.count(dkrt_sdk::logk::PROJECT_LOADED);
    let (status, _) = summary(&mut r, 20);
    assert_eq!(status, E_AGAIN);
    assert_eq!(
        summary(&mut r, 20).0,
        E_AGAIN,
        "asking again does not answer early"
    );
    r.settle();
    let (status, s) = summary(&mut r, 20);
    assert_eq!(status, OK);
    assert_eq!((s.slot, s.stamp, s.counter), (20, 0x2020, 4));
    assert_eq!(&s.name[..s.name_len as usize], b"TWENTY");
    assert_eq!(
        ids(&s),
        [
            (b"a/x".to_vec(), 0xa1, 2, MODULE_LOADED),
            (b"b/y".to_vec(), 0xb2, 1, MODULE_UNKNOWN)
        ]
    );
    // reading it for the manager binds nothing and logs no load
    assert_eq!(r.p.slot(), 7);
    assert_eq!(r.f.count(dkrt_sdk::logk::PROJECT_LOADED), loaded_events);
}

#[test]
fn get_says_not_found_for_a_slot_with_no_usable_file() {
    let mut r = rig_with_files();
    r.rec(30, b"EMPTY", 0x30);
    r.rec(31, b"STALE", 0x31);
    r.f.store.insert(
        path(31, b"STALE"),
        file_bytes(31, b"STALE", 0x7777, 1, &[(b"test/owner/k", b"v")]),
    );
    r.load(7);
    // a slot with a record and no file, and one whose file has another stamp: read, then not found
    for slot in [30, 31] {
        assert_eq!(summary(&mut r, slot).0, E_AGAIN);
        r.settle();
        assert_eq!(summary(&mut r, slot).0, E_NOT_FOUND, "slot {slot}");
    }
    // a slot the firmware does not list is not found at once, nothing is queued
    assert_eq!(summary(&mut r, 99).0, E_NOT_FOUND);
    assert_eq!(summary(&mut r, 200).0, E_NOT_FOUND);
    assert!(!r.p.busy(&r.job));
}

#[test]
fn one_summary_is_kept_and_a_request_for_another_slot_replaces_it() {
    let mut r = rig_with_files();
    r.load(7);
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    r.settle();
    assert_eq!(summary(&mut r, 22).0, E_AGAIN);
    r.settle();
    let reads = r.f.reads;
    let (status, s) = summary(&mut r, 22);
    assert_eq!((status, s.slot, s.counter), (OK, 22, 9));
    assert_eq!(
        r.f.reads, reads,
        "the kept summary is answered without a read"
    );
    // slot 20 was replaced: it is read again
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    r.settle();
    assert_eq!(summary(&mut r, 20).1.counter, 4);
    // a request that was not started yet is replaced as well
    assert_eq!(summary(&mut r, 22).0, E_AGAIN);
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    r.settle();
    assert_eq!(summary(&mut r, 20).0, OK);
}

#[test]
fn a_get_while_the_stage_or_the_loader_is_busy_waits_for_its_turn() {
    let mut r = rig_with_files();
    r.load(7);
    r.settle();
    // a live operation stages: the read waits for it
    r.f.live_op = 3;
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    let reads = r.f.reads;
    for _ in 0..10 {
        r.pass();
    }
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    assert_eq!(r.f.reads, reads, "nothing was read while the loader stages");
    r.f.live_op = 0;
    r.settle();
    assert_eq!(summary(&mut r, 20).0, OK);
    // a save that waits for a busy store holds the stage: the read starts after it
    assert_eq!(r.put(b"k", b"v"), OK);
    r.hook_save(7);
    r.f.busy = true;
    for _ in 0..4 {
        r.pass();
    }
    assert_eq!(summary(&mut r, 22).0, E_AGAIN);
    let reads = r.f.reads;
    for _ in 0..10 {
        r.pass();
    }
    assert_eq!(r.f.reads, reads, "the busy save runs first");
    r.f.busy = false;
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert!(r.f.store.contains_key(&path(7, b"SONG")), "the save ended");
    assert_eq!(summary(&mut r, 22).0, OK);
}

#[test]
fn a_change_of_the_slots_file_drops_its_kept_summary() {
    let mut r = rig_with_files();
    r.load(7);
    assert_eq!(summary(&mut r, 20).0, E_AGAIN);
    r.settle();
    assert_eq!(summary(&mut r, 20).0, OK);
    r.hook_clear(20);
    r.settle();
    assert_eq!(
        summary(&mut r, 20).0,
        E_AGAIN,
        "the old summary is not given"
    );
    r.settle();
    assert_eq!(summary(&mut r, 20).0, E_NOT_FOUND);
}
