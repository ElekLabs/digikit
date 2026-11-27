//! The module list: restored after a load.
use crate::*;
use dkrt_sdk::logk::{PROJECT_LOADED, PROJECT_MODULE};
use dkrt_sdk::{E_BUSY, E_CARD_BUSY, E_FORMAT, E_PLAN_FIT, E_READ, E_SEL_COUNT, E_SEL_PATH};
use dkrt_svc_project::engine::*;

#[test]
fn a_load_queues_the_live_loads_of_the_listed_modules_in_order_after_the_install_and_adds_only() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    // a module the list does not name stays loaded
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    r.load(7);
    assert_eq!(
        r.f.live_loads,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/fx/b-y.dkm".to_vec()
        ]
    );
    assert_eq!(
        module_events(&r),
        [
            (1, 0, 0, id_hash(b"a/x")),
            (2, 0, 0, id_hash(b"a/x")),
            (1, 0, 1, id_hash(b"b/y")),
            (2, 0, 1, id_hash(b"b/y"))
        ]
    );
    assert_eq!(
        r.f.loaded_ids(),
        [b"c/z".to_vec(), b"a/x".to_vec(), b"b/y".to_vec()],
        "the extra module is still loaded: the restore never unloads"
    );
    assert_eq!(
        r.f.selection,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/fx/b-y.dkm".to_vec()
        ],
        "a load the service asked for is saved in the selection like one from the menu"
    );
    assert!(r.f.warnings.is_empty());
    // the loads came after the install (LOADED comes first in the log)
    let loaded_at = r.f.logs.iter().position(|l| l.0 == PROJECT_LOADED).unwrap();
    let first_module = r.f.logs.iter().position(|l| l.0 == PROJECT_MODULE).unwrap();
    assert!(loaded_at < first_module);
}

#[test]
fn a_listed_module_that_is_loaded_already_is_not_asked_for_and_a_stock_project_asks_for_nothing() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.rec(8, b"PLAIN", 0x88);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.loaded
        .push((b"a/x".to_vec(), b"modules/machine/a-x.dkm".to_vec()));
    r.load(7);
    assert!(r.f.live_loads.is_empty() && module_events(&r).is_empty());
    // a project with no .dkp: nothing is queued, whatever is loaded
    r.f.loaded.clear();
    r.load(8);
    assert!(r.f.live_loads.is_empty() && module_events(&r).is_empty());
}

#[test]
fn a_load_refused_while_the_transport_plays_waits_for_the_stop_and_counts_for_no_try() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.playing = true;
    r.hook_slot(7);
    // many refusals, far more than a retry count would allow: one every PLAY_WAIT_MS
    for _ in 0..12 {
        for _ in 0..6 {
            r.pass();
        }
        r.f.now += PLAY_WAIT_MS + 1;
    }
    assert!(r.f.loaded.is_empty() && r.f.warnings.is_empty());
    assert!(
        module_events(&r).iter().all(|e| e.0 == 1),
        "queued again and again, never refused"
    );
    // the last wait ran out: one more refusal, then the wait begins again
    for _ in 0..6 {
        r.pass();
    }
    let asked = r.f.live_loads.len();
    assert!(asked >= 10, "{asked}");
    // within the wait nothing is asked
    for _ in 0..10 {
        r.pass();
    }
    assert_eq!(r.f.live_loads.len(), asked);
    // the transport stops
    r.f.playing = false;
    r.f.now += PLAY_WAIT_MS + 1;
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(module_events(&r).last().unwrap().0, 2);
}

#[test]
fn a_module_that_is_not_on_the_card_is_one_log_line_and_one_warning_and_the_others_still_load() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/gone", 1), (b"b/y", 2)]),
    );
    r.f.card("fx", "b/y");
    r.load(7);
    assert_eq!(r.p.slot(), 7, "the project loads");
    assert_eq!(r.f.warnings, [(ModuleWarning::Missing, b"a/gone".to_vec())]);
    assert_eq!(
        module_events(&r)[0],
        (3, 0, 0, id_hash(b"a/gone")),
        "MISSING for the first, then the second goes on"
    );
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
    assert_eq!(r.f.live_loads, [b"modules/fx/b-y.dkm".to_vec()]);
}

#[test]
fn a_load_the_runtime_refuses_is_a_warning_and_a_log_line_and_the_next_one_continues() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    // the job of the first ends with a refusal; the second works
    r.f.job_results.push_back(E_PLAN_FIT);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::NoFit, b"a/x".to_vec())]);
    let events = module_events(&r);
    assert_eq!(events[0], (1, 0, 0, id_hash(b"a/x")));
    assert_eq!(events[1], (4, E_PLAN_FIT, 0, id_hash(b"a/x")));
    assert_eq!(events[3], (2, 0, 1, id_hash(b"b/y")));
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
    assert_eq!(
        r.f.selection,
        [b"modules/fx/b-y.dkm".to_vec()],
        "a refused module is not saved in the selection"
    );
}

#[test]
fn a_refusal_that_is_not_a_fit_refusal_is_the_other_warning() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 1, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.job_results.push_back(E_FORMAT);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
}

#[test]
fn a_project_change_while_the_restore_runs_replaces_the_pending_list() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.rec(8, b"OTHER", 0x88);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.store.insert(
        path(8, b"OTHER"),
        file_with_modules(8, b"OTHER", 0x88, 2, &[(b"c/z", 3)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.card("tool", "c/z");
    // the first load is queued and stays running
    r.f.hold_jobs = true;
    r.hook_slot(7);
    for _ in 0..40 {
        r.pass();
    }
    assert_eq!(r.f.live_loads, [b"modules/machine/a-x.dkm".to_vec()]);
    // another project becomes current
    r.hook_slot(8);
    for _ in 0..40 {
        r.pass();
    }
    // the job of the old list ends in the runtime; the service goes on with the new list only
    r.f.hold_jobs = false;
    r.settle();
    assert_eq!(
        r.f.live_loads,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/tool/c-z.dkm".to_vec()
        ],
        "b/y of the first project is never asked for"
    );
    assert!(r.f.loaded_ids().contains(&b"c/z".to_vec()));
    assert!(!r.f.loaded_ids().contains(&b"b/y".to_vec()));
}

#[test]
fn a_store_that_is_busy_when_the_file_is_searched_asks_the_same_entry_again_and_is_never_missing() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.find_script.extend([E_BUSY, E_BUSY, E_BUSY]);
    r.load(7);
    assert!(r.f.warnings.is_empty(), "{:?}", r.f.warnings);
    assert_eq!(r.f.finds, 4);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
}

#[test]
fn a_store_that_cannot_be_read_is_a_refusal_and_not_a_missing_module() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.find_script.push_back(E_READ);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
    assert_eq!(module_events(&r)[0], (4, E_READ, 0, id_hash(b"a/x")));
    assert_eq!(
        r.f.loaded_ids(),
        [b"b/y".to_vec()],
        "the next entry goes on"
    );
}

#[test]
fn the_store_is_not_read_while_a_live_job_stages() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.hook_slot(7);
    // the project installs, then a live job stages for many passes: no search (the fake asserts it)
    while r.f.count(PROJECT_LOADED) == 0 {
        r.pass();
    }
    r.f.live_op = 5;
    for _ in 0..30 {
        r.pass();
    }
    assert_eq!(r.f.finds, 0);
    assert!(r.f.loaded.is_empty());
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
}

#[test]
fn a_busy_card_is_asked_again_a_few_times_and_then_a_refusal() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    // twice busy, then it works
    r.f.live_answers.extend([E_CARD_BUSY, E_CARD_BUSY]);
    r.hook_slot(7);
    for _ in 0..20 {
        r.pass();
        r.f.now += CARD_WAIT_MS + 1;
    }
    assert!(r.f.warnings.is_empty());
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(r.f.live_asked, 3);
    // busy for ever: CARD_TRIES_MAX more asks, then one warning
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.live_answers.extend([E_CARD_BUSY; 4]);
    r.hook_slot(7);
    for _ in 0..40 {
        r.pass();
        r.f.now += CARD_WAIT_MS + 1;
    }
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
    assert_eq!(
        r.f.live_asked,
        4 + 1,
        "the first four asks of a/x, then b/y"
    );
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
}

#[test]
fn a_retry_pass_reads_no_hash_and_searches_the_store_once_per_entry() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    // the runtime is busy for ten asks
    r.f.live_answers.extend([E_BUSY; 10]);
    r.hook_slot(7);
    r.f.hash_reads.set(0);
    for _ in 0..30 {
        r.pass();
    }
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"c/z".to_vec(), b"a/x".to_vec()]);
    assert_eq!(r.f.finds, 1, "the file of an entry is searched once");
    assert_eq!(
        r.f.hash_reads.get(),
        0,
        "the loaded ids are compared without a store read"
    );
}

#[test]
fn a_module_whose_path_the_selection_cannot_hold_loads_and_is_told_as_not_kept() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    // 63 bytes: the live load takes it, the selection does not
    let kind = "k".repeat(63 - "modules//a-x.dkm".len());
    r.f.card(&kind, "a/x");
    r.load(7);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()], "loaded live");
    assert!(r.f.selection.is_empty());
    assert_eq!(r.f.warnings, [(ModuleWarning::NotKept, b"a/x".to_vec())]);
    let events = module_events(&r);
    assert_eq!(events[1], (5, E_SEL_PATH, 0, id_hash(b"a/x")));
    assert_eq!(events.len(), 3, "queued, not kept, done");
}

#[test]
fn a_full_selection_is_told_as_not_kept_after_the_load() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.selection_answer = Some(E_SEL_COUNT);
    r.load(7);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(r.f.warnings, [(ModuleWarning::NotKept, b"a/x".to_vec())]);
    assert_eq!(module_events(&r).last().unwrap().1, E_SEL_COUNT);
}
