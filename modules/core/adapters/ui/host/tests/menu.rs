//! The logic of core/digikit-menu on the fake <dk/platform/ui.h>: the menu's own C source (modules/core/adapters/ui/host/tests/c/menu_fake.c) with a fake runtime.
use dkui_host::driver::*;
use dkui_host::*;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn fk_reset();
    fn fk_add(path: *const c_char, flags: u32, arena: u32, fx_slots: u32, machines: u32);
    fn fk_select(path: *const c_char);
    fn fk_scan_result(r: c_int);
    fn fk_boot_safe();
    fn fk_loaded(path: *const c_char, name: *const c_char);
    fn fk_fx(id: *const c_char, allowed: u32, saved: u32);
    fn fk_loaded_menu(path: *const c_char, name: *const c_char);
    fn fk_settings_runs() -> c_int;
    fn fk_settings_acts() -> c_int;
    fn fk_send_slots(n: u32);
    fn fk_restart_result(r: c_int);
    fn fk_open_digikit() -> c_int;
    fn fk_restarts() -> c_int;
    fn fk_selection_sets() -> c_int;
    fn fk_set_count() -> c_int;
    fn fk_set_path(i: c_int) -> *const c_char;
    fn fk_flushes() -> c_int;
    fn fk_tick(i: c_int) -> c_int;
    fn fk_live_script(i: c_int, queue_result: c_int, job_result: c_int);
    fn fk_live_waits(n: c_int);
    fn fk_poll();
    fn fk_poll_on() -> c_int;
    fn fk_live_calls() -> c_int;
    fn fk_live_op(i: c_int) -> c_int;
    fn fk_live_path(i: c_int) -> *const c_char;
    fn fk_boot_needs(k: c_int, name: *const c_char);
    fn fk_set_handle(k: c_int) -> usize;
    fn fk_say_poll();
    fn fk_say_twice();
    fn fk_boot_rename(k: c_int, name: *const c_char);
    fn fk_boot_count(n: u32);
    fn fk_poll_slots(n: c_int);
    fn fk_settings_poll();
    fn fk_set_open(k: c_int) -> c_int;
    fn fk_menu_held() -> c_int;
    fn fk_unload_module(path: *const c_char);
    fn fk_set_refresh(k: c_int);
}

// dkrt_live ops
const LIVE_LOAD: c_int = 1;
const LIVE_UNLOAD: c_int = 2;

/// UI-loop passes until the batch has ended (the callback removes itself).
fn poll_until_idle() {
    for _ in 0..200 {
        if unsafe { fk_poll_on() } == 0 {
            return;
        }
        unsafe { fk_poll() };
    }
    panic!("the live batch did not end");
}

fn live_call(i: c_int) -> (c_int, String) {
    unsafe { (fk_live_op(i), cstr(fk_live_path(i))) }
}

const COMPAT: u32 = 1;
const NEEDS: u32 = 2;
const LOADED: u32 = 4;
const OK: u32 = COMPAT | NEEDS;
const KB: u32 = 1024;
// the rows of the DIGIKIT menu
const MODULES: usize = 3;

fn add(path: &str, flags: u32, arena_kb: u32, fx_slots: u32, machines: u32) {
    let p = CString::new(path).unwrap();
    unsafe { fk_add(p.as_ptr(), flags, arena_kb * KB, fx_slots, machines) }
}

fn select(path: &str) {
    let p = CString::new(path).unwrap();
    unsafe { fk_select(p.as_ptr()) }
}

fn loaded(path: &str, name: &str) {
    let (p, n) = (CString::new(path).unwrap(), CString::new(name).unwrap());
    unsafe { fk_loaded(p.as_ptr(), n.as_ptr()) }
}

/// A loaded module that has a menu function.
fn loaded_menu(path: &str, name: &str) {
    let (p, n) = (CString::new(path).unwrap(), CString::new(name).unwrap());
    unsafe { fk_loaded_menu(p.as_ptr(), n.as_ptr()) }
}

fn fx(id: &str, allowed: u32, saved: u32) {
    let i = CString::new(id).unwrap();
    unsafe { fk_fx(i.as_ptr(), allowed, saved) }
}

fn cstr(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A fresh session with DIGIKIT open. Fill the fake store first with `store`.
fn digikit() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe { fk_reset() };
    g
}

fn open_digikit() {
    assert_eq!(unsafe { fk_open_digikit() }, 0);
}

/// Opens MODULES from the menu.
fn open_modules() {
    open_digikit();
    press_yes(MODULES);
    assert_eq!(title(), "MODULES");
}

/// Three modules: a tool, an effect (loaded, with a route) and a machine too big for the arena.
fn store_of_three() {
    add("modules/tool.dkm", OK, 20, 0, 0);
    add("modules/gain-fx.dkm", OK | LOADED, 100, 1, 0);
    add("modules/huge.dkm", OK, 900, 0, 1);
}

#[test]
fn digikit_menu_rows() {
    let _g = digikit();
    open_digikit();
    assert_eq!(title(), "DIGIKIT");
    assert_eq!(render(), ["ABOUT", "HEALTH", "MEMORY", "MODULES"]);
}

#[test]
fn modules_list_has_one_row_per_file_with_kind_and_size() {
    let _g = digikit();
    store_of_three();
    select("modules/gain-fx.dkm");
    open_modules();
    // name (12), kind (4), size right aligned; the check mark starts from the saved selection
    assert_eq!(
        render(),
        [
            "  TOOL         TOOL   20K",
            "* GAIN-FX      FX    100K",
            "  HUGE         MACH  900K",
        ]
    );
}

/// Pixels of a row label in the list font: a letter, a digit, '-' and the padding glyph 0x01 take 4, a space 3, '.' 2 and '~' 5.
fn px(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            ' ' => 3,
            '.' => 2,
            '~' => 5,
            _ => 4,
        })
        .sum()
}

#[test]
fn modules_columns_line_up_in_pixels() {
    let _g = digikit();
    add("modules/bd.dkm", OK, 2, 0, 1);
    add("modules/bootscreen.dkm", OK, 7, 0, 0);
    add("modules/send.dkm", OK, 1250, 1, 0);
    open_modules();
    let rows = labels();
    assert_eq!(rows.len(), 3);
    for row in &rows {
        // the kind starts after 13 letter widths, the size ends after 23, and the row fits
        let kind = ["MACH", "TOOL", "FX"]
            .iter()
            .filter_map(|k| row.rfind(k))
            .max()
            .unwrap();
        assert_eq!(px(&row[..kind]), 13 * 4, "{row:?}");
        assert_eq!(px(row), 23 * 4, "{row:?}");
        assert!(row.len() < DK_UI_ROW, "{row:?}");
    }
}

#[test]
fn a_modules_row_that_needs_more_characters_still_fits() {
    let _g = digikit();
    // a space in the name and a 5-character size: exact padding would take 24 characters
    add("modules/a b.dkm", OK, 1250, 0, 1);
    open_modules();
    let row = &labels()[0];
    assert!(row.ends_with("1250K"), "{row:?}");
    assert_eq!(px(row), 23 * 4, "{row:?}");
}

#[test]
fn selected_modules_that_are_loaded_show_a_check_mark() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/gain-fx.dkm", OK | LOADED, 100, 1, 0);
    select("modules/tool.dkm");
    select("modules/gain-fx.dkm");
    open_modules();
    assert_eq!(
        render(),
        ["* TOOL         TOOL   20K", "* GAIN-FX      FX    100K"]
    );
}

#[test]
fn a_selected_module_that_failed_to_load_shows_a_dash() {
    let _g = digikit();
    add("modules/tool.dkm", OK, 20, 0, 0); // selected, not loaded
    add("modules/gain-fx.dkm", OK | LOADED, 100, 1, 0);
    add("modules/huge.dkm", OK, 900, 0, 1); // not selected
    select("modules/tool.dkm");
    select("modules/gain-fx.dkm");
    open_modules();
    // the marks are text in a column of their own; the check mark stays off for the dash
    assert_eq!(
        render(),
        [
            "  -TOOL        TOOL   20K",
            "*  GAIN-FX     FX    100K",
            "   HUGE        MACH  900K",
        ]
    );
    // YES unselects the dash row and the next YES selects it again
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 0);
    assert!(render()[0].starts_with("   TOOL"), "{:?}", render());
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 1);
    assert!(render()[0].starts_with("  -TOOL"), "{:?}", render());
}

#[test]
fn safe_mode_has_a_header_and_dashes_for_the_selected_modules() {
    let _g = digikit();
    add("modules/tool.dkm", OK, 20, 0, 0);
    add("modules/gain-fx.dkm", OK, 100, 1, 0);
    select("modules/tool.dkm");
    unsafe { fk_boot_safe() };
    open_digikit();
    press_yes(4); // below the SAFE row
    assert_eq!(title(), "MODULES");
    assert_eq!(
        render(),
        [
            "  SAFE MODE: NOT LOADED",
            "  -TOOL        TOOL   20K",
            "   GAIN-FX     FX    100K",
        ]
    );
    // the cursor starts on the first module; the header is no module: YES and RIGHT do nothing there
    press_yes(0);
    assert_eq!(render()[0], "  SAFE MODE: NOT LOADED");
    assert_eq!((unsafe { fk_tick(0) }, unsafe { fk_tick(1) }), (1, 0));
    press_right(0);
    assert_eq!(title(), "MODULES");
    // the rows after it are modules 0 and 1
    press_yes(2);
    assert_eq!((unsafe { fk_tick(0) }, unsafe { fk_tick(1) }), (1, 1));
    assert!(render()[2].starts_with("*  GAIN-FX"), "{:?}", render());
    press_right(1);
    assert_eq!(render(), ["UNLOAD", "INFO"]);
}

#[test]
fn long_module_rows_keep_the_full_name_kind_and_size() {
    let _g = digikit();
    // 63 bytes including the directory and extension, the longest store path.
    let name = "a".repeat(51);
    add(&format!("modules/{name}.dkm"), OK, 10, 1, 0);
    open_modules();
    let row = &labels()[0];
    let (prefix, size) = row.split_once("FX").unwrap();
    assert_eq!(prefix.trim_end_matches([' ', '\x01']), name.to_uppercase());
    assert!(prefix.ends_with([' ', '\x01']));
    assert_eq!(size.trim_start_matches([' ', '\x01']), "10K");
}

#[test]
fn empty_and_missing_store_texts() {
    let _g = digikit();
    open_modules();
    assert_eq!(render(), ["NO MODULES ON CARD"]);
    press_no();
    press_no();
    unsafe { fk_scan_result(-1) };
    open_modules();
    assert_eq!(render(), ["NO STORE"]);
}

#[test]
fn yes_toggles_the_mark_and_nothing_is_saved_yet() {
    let _g = digikit();
    store_of_three();
    select("modules/gain-fx.dkm"); // loaded and saved
    open_modules();
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 1);
    assert!(render()[0].starts_with("* TOOL"));
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 0);
    assert_eq!(unsafe { fk_selection_sets() }, 0);
}

#[test]
fn a_module_that_does_not_fit_is_refused_with_a_confirm() {
    let _g = digikit();
    store_of_three();
    select("modules/gain-fx.dkm"); // loaded and saved
    open_modules();
    press_yes(2);
    // the mark went back off
    assert_eq!(unsafe { fk_tick(2) }, 0);
    assert!(render()[2].starts_with("  HUGE"));
    let (kind, title, text, _) = dialog().expect("a dialog");
    assert_eq!(kind, DialogKind::Confirm);
    assert_eq!(title, "HUGE DOES NOT FIT");
    assert_eq!(text, "CPU 900K > 412K FREE");
    // it is a notice: either answer closes it and changes nothing
    answer_confirm(true);
    assert!(dialog().is_none());
    press_no();
    assert_eq!(depth(), 1); // no changes pending: NO went back at once
}

#[test]
fn fit_counts_what_is_checked_already() {
    let _g = digikit();
    add("modules/a.dkm", OK, 300, 0, 1);
    add("modules/b.dkm", OK, 300, 0, 1);
    open_modules();
    press_yes(0);
    assert!(dialog().is_none());
    press_yes(1); // 300K + 300K > 512K
    let (_, title, _, _) = dialog().expect("a dialog");
    assert_eq!(title, "B DOES NOT FIT");
    answer_confirm(false);
    // unchecking the first makes room
    press_yes(0);
    press_yes(1);
    assert!(dialog().is_none());
    assert_eq!((unsafe { fk_tick(0) }, unsafe { fk_tick(1) }), (0, 1));
}

#[test]
fn a_module_for_another_runtime_is_not_compatible() {
    let _g = digikit();
    add("modules/foreign.dkm", NEEDS, 10, 0, 1);
    open_modules();
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 0);
    let (_, title, text, _) = dialog().expect("a dialog");
    assert_eq!(title, "FOREIGN NOT COMPATIBLE");
    assert!(text.contains("ANOTHER DEVICE OR RUNTIME"));
}

#[test]
fn the_side_panel_has_load_route_and_info() {
    let _g = digikit();
    store_of_three();
    loaded("modules/gain-fx.dkm", "test/gain-fx");
    fx("test/gain-fx", 3, 1);
    select("modules/gain-fx.dkm");
    open_modules();
    // a tool and a machine have no route; an effect that is loaded has
    press_right(0);
    assert_eq!(render(), ["LOAD", "INFO"]);
    choose_panel_row(0); // LOAD checks the row
    assert_eq!(unsafe { fk_tick(0) }, 1);
    press_right(0);
    assert_eq!(render(), ["UNLOAD", "INFO"]);
    press_no();
    press_right(1);
    assert_eq!(render(), ["UNLOAD", "ROUTE...", "INFO"]);
    choose_panel_row(0); // UNLOAD
    assert_eq!(unsafe { fk_tick(1) }, 0);
    // LOAD on a row that does not fit is refused as YES is
    press_right(2);
    choose_panel_row(0);
    assert_eq!(dialog().unwrap().1, "HUGE DOES NOT FIT");
}

#[test]
fn a_loaded_module_with_a_menu_gets_settings_in_the_side_panel() {
    let _g = digikit();
    store_of_three();
    loaded_menu("modules/tool.dkm", "digikit/tool");
    loaded("modules/gain-fx.dkm", "test/gain-fx");
    fx("test/gain-fx", 3, 1);
    select("modules/tool.dkm");
    select("modules/gain-fx.dkm");
    open_modules();
    // only the module with a menu function has the row; a loaded effect without one does not
    press_right(0);
    assert_eq!(render(), ["UNLOAD", "SETTINGS...", "INFO"]);
    press_no();
    press_right(1);
    assert_eq!(render(), ["UNLOAD", "ROUTE...", "INFO"]);
    press_no();
    // a module that is in the store but not loaded has none
    press_right(2);
    assert_eq!(render(), ["LOAD", "INFO"]);
    press_no();
    // the module's menu function runs only when the user opens SETTINGS..., and again at each open
    assert_eq!(unsafe { fk_settings_runs() }, 0);
    press_right(0);
    choose_panel_row(1);
    assert_eq!(title(), "TOOL"); // the module name without its author
    assert!(is_wide(), "module settings use the full-width page");
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert_eq!(unsafe { fk_settings_runs() }, 1);
    press_yes(1);
    assert_eq!(unsafe { fk_settings_acts() }, 1);
    press_no();
    press_right(0);
    choose_panel_row(1);
    assert_eq!(unsafe { fk_settings_runs() }, 2);
    assert_eq!(depth(), 3); // DIGIKIT, MODULES, the settings
}

#[test]
fn a_module_settings_page_has_the_full_name_as_its_title() {
    let _g = digikit();
    store_of_three();
    loaded_menu("modules/tool.dkm", "digikit/a-long-module-name-here");
    select("modules/tool.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    // no cut to a few characters: the wide title bar holds the whole name (the firmware cuts it only past about 28)
    assert_eq!(title(), "A-LONG-MODULE-NAME-HERE");
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert!(is_wide());
    // the DIGIKIT menu and the MODULES list keep their own pages
    press_no();
    assert!(!is_wide());
}

#[test]
fn info_page_shows_the_module() {
    let _g = digikit();
    store_of_three();
    open_modules();
    press_right(0);
    choose_panel_row(1);
    assert_eq!(title(), "INFO");
    let r = render();
    assert_eq!(r[0], "TOOL");
    assert_eq!(r[1], "VERSION 0.1.0");
    assert_eq!(r[2], "SIZE 20K");
    assert!(r.contains(&"FITS".to_string()), "{r:?}");
    assert_eq!(r.last().unwrap(), "STATUS NOT LOADED");
}

#[test]
fn route_choice_is_limited_to_what_the_effect_allows() {
    let _g = digikit();
    add("modules/both.dkm", OK | LOADED, 10, 1, 0);
    add("modules/master-only.dkm", OK | LOADED, 10, 1, 0);
    loaded("modules/both.dkm", "test/both");
    loaded("modules/master-only.dkm", "test/master-only");
    fx("test/both", 3, 1);
    fx("test/master-only", 1, 1);
    unsafe { fk_send_slots(2) };
    open_modules();
    press_right(0);
    choose_panel_row(1);
    let (kind, title, _, items) = dialog().unwrap();
    assert_eq!((kind, title.as_str()), (DialogKind::Choice, "ROUTE"));
    assert_eq!(items, ["SEND", "MASTER"]);
    pick_choice(0);
    // a change of the route is a pending change
    press_no();
    let (_, title, text, _) = dialog().unwrap();
    assert_eq!(
        (title.as_str(), text.as_str()),
        ("APPLY & RESTART?", "1 CHANGE")
    );
    answer_confirm(false);
    // the second effect allows the master only
    open_modules();
    press_right(1);
    choose_panel_row(1);
    assert_eq!(dialog().unwrap().3, ["MASTER"]);
}

#[test]
fn send_is_not_offered_when_every_send_slot_is_taken() {
    let _g = digikit();
    add("modules/a.dkm", OK | LOADED, 10, 1, 0);
    add("modules/b.dkm", OK | LOADED, 10, 1, 0);
    loaded("modules/a.dkm", "test/a");
    loaded("modules/b.dkm", "test/b");
    fx("test/a", 3, 2); // on the send bus already
    fx("test/b", 3, 1); // on the master
    unsafe { fk_send_slots(1) };
    open_modules();
    press_right(1);
    choose_panel_row(1);
    assert_eq!(dialog().unwrap().3, ["MASTER"]);
    press_no(); // NO closes a choice without an answer
    assert!(dialog().is_none());
}

#[test]
fn no_without_changes_just_goes_back() {
    let _g = digikit();
    store_of_three();
    select("modules/tool.dkm");
    open_modules();
    assert_eq!(depth(), 2);
    press_no();
    assert_eq!((depth(), title().as_str()), (1, "DIGIKIT"));
    assert!(dialog().is_none());
    // a mark that was set and unset again is no change
    open_modules();
    press_yes(1);
    press_yes(1);
    press_no();
    assert_eq!(depth(), 1);
    assert!(dialog().is_none());
}

#[test]
fn no_with_changes_asks_to_apply_and_runs_it_live() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/gain-fx.dkm", OK | LOADED, 100, 1, 0);
    add("modules/huge.dkm", OK, 900, 0, 1);
    select("modules/tool.dkm");
    open_modules();
    press_yes(0); // unchecks the saved one
    press_yes(1); // checks another
    press_no();
    // the list stays and the question is asked
    assert_eq!(title(), "MODULES");
    let (kind, title, text, _) = dialog().unwrap();
    assert_eq!(
        (kind, title.as_str(), text.as_str()),
        (DialogKind::Confirm, "APPLY 2 CHANGES?", "NO RESTART")
    );
    answer_confirm(true);
    assert_eq!(unsafe { fk_selection_sets() }, 1);
    assert_eq!(unsafe { fk_set_count() }, 1);
    assert_eq!(cstr(unsafe { fk_set_path(0) }), "modules/gain-fx.dkm");
    assert_eq!(unsafe { fk_flushes() }, 1);
    // gain-fx is loaded already: the only job is the unload of tool, and the device does not restart
    poll_until_idle();
    assert_eq!(unsafe { fk_live_calls() }, 1);
    assert_eq!(live_call(0), (LIVE_UNLOAD, "modules/tool.dkm".to_string()));
    assert_eq!(unsafe { fk_restarts() }, 0);
}

#[test]
fn a_batch_unloads_first_then_loads_and_the_row_reads_loading() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/a.dkm", OK, 20, 0, 0);
    add("modules/b.dkm", OK, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_live_waits(1) };
    open_modules();
    press_yes(2); // the loads are checked before the unload: the batch still unloads first
    press_yes(1);
    press_yes(0);
    press_no();
    assert_eq!(dialog().unwrap().1, "APPLY 3 CHANGES?");
    answer_confirm(true);
    assert_eq!(unsafe { fk_restarts() }, 0);
    assert_eq!(unsafe { fk_poll_on() }, 1);
    unsafe { fk_poll() }; // the unload of tool is queued: its row reads UNLOADING
    assert!(render()[0].contains("UNLOADING"), "{:?}", render());
    // the list is read-only until the batch ends: a key changes nothing and NO does not leave
    press_yes(0);
    assert_eq!(unsafe { fk_tick(0) }, 0);
    press_no();
    assert_eq!((title().as_str(), depth()), ("MODULES", 2));
    assert!(dialog().is_none());
    unsafe { fk_poll() }; // still running
    unsafe { fk_poll() }; // finished: the list is scanned again, the next job follows
    assert!(!render()[0].contains("UNLOADING"), "{:?}", render());
    unsafe { fk_poll() };
    assert!(render()[1].contains("LOADING"), "{:?}", render());
    poll_until_idle();
    assert_eq!(unsafe { fk_live_calls() }, 3);
    assert_eq!(live_call(0), (LIVE_UNLOAD, "modules/tool.dkm".to_string()));
    assert_eq!(live_call(1), (LIVE_LOAD, "modules/a.dkm".to_string()));
    assert_eq!(live_call(2), (LIVE_LOAD, "modules/b.dkm".to_string()));
    // the batch ended: the list is closed, and a new one shows what is loaded now
    assert_eq!(depth(), 1);
    open_modules();
    assert_eq!(
        render().iter().map(|r| &r[..2]).collect::<Vec<_>>(),
        ["  ", "* ", "* "]
    );
    assert_eq!(unsafe { fk_restarts() }, 0);
}

/// One job of the batch (the unload of tool, then a load) is refused or fails with `code`; the dialogs that follow.
fn refused_batch(queue_result: c_int, job_result: c_int) {
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/a.dkm", OK, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_live_script(0, queue_result, job_result) };
    open_modules();
    press_yes(0);
    press_yes(1);
    press_no();
    answer_confirm(true);
    poll_until_idle();
    // the batch ends at the first job that did not work: the load of a is not tried
    assert_eq!(unsafe { fk_live_calls() }, 1);
}

#[test]
fn a_refused_job_shows_not_applied_and_offers_the_restart() {
    let _g = digikit();
    refused_batch(0, -83);
    let (kind, title, text, _) = dialog().unwrap();
    assert_eq!(
        (kind, title.as_str(), text.as_str()),
        (DialogKind::Confirm, "TOOL NOT APPLIED", "STOP PLAYBACK")
    );
    answer_confirm(true);
    let (_, title, text, _) = dialog().expect("the restart question");
    assert_eq!(
        (title.as_str(), text.as_str()),
        ("RESTART TO APPLY?", "THE CHANGE IS SAVED")
    );
    assert_eq!(unsafe { fk_restarts() }, 0);
    answer_confirm(true);
    assert_eq!(unsafe { fk_restarts() }, 1);
}

#[test]
fn a_missing_service_is_not_applied_and_no_to_the_restart_leaves_the_device_running() {
    let _g = digikit();
    refused_batch(-73, 0); // E_MISSING at the queue
    let (_, dtitle, text, _) = dialog().unwrap();
    assert_eq!(
        (dtitle.as_str(), text.as_str()),
        ("TOOL NOT APPLIED", "NO SERVICE")
    );
    answer_confirm(false);
    assert_eq!(dialog().unwrap().1, "RESTART TO APPLY?");
    answer_confirm(false);
    assert!(dialog().is_none());
    assert_eq!(unsafe { fk_restarts() }, 0);
    // the selection is saved; the list is back to the scan of what is loaded
    assert_eq!(unsafe { fk_selection_sets() }, 1);
    assert_eq!(title(), "MODULES");
}

#[test]
fn the_text_of_each_live_refusal() {
    for (code, text) in [
        (-84, "HAS MENUS"),
        (-85, "ON AN FX ROUTE"),
        (-86, "KEYS CHANGED"),
        (-87, "RESTART DEVICE"),
        (-53, "UPLOAD OPEN"),
    ] {
        let _g = digikit();
        refused_batch(0, code);
        assert_eq!(dialog().unwrap().2, text, "code {code}");
    }
}

#[test]
fn a_route_change_restarts_directly() {
    let _g = digikit();
    add("modules/both.dkm", OK | LOADED, 10, 1, 0);
    loaded("modules/both.dkm", "test/both");
    fx("test/both", 3, 1);
    unsafe { fk_send_slots(2) };
    select("modules/both.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    pick_choice(0); // SEND
    press_no();
    assert_eq!(dialog().unwrap().1, "APPLY & RESTART?");
    answer_confirm(true);
    assert_eq!(
        (unsafe { fk_restarts() }, unsafe { fk_live_calls() }),
        (1, 0)
    );
}

#[test]
fn leaving_safe_mode_restarts_directly() {
    let _g = digikit();
    add("modules/tool.dkm", OK, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_boot_safe() };
    open_digikit();
    answer_confirm(false); // the safe-mode notice
    press_yes(MODULES + 1); // the SAFE row comes first
    press_no();
    let (_, title, text, _) = dialog().unwrap();
    assert_eq!(
        (title.as_str(), text.as_str()),
        ("APPLY & RESTART?", "LEAVES SAFE MODE")
    );
    answer_confirm(true);
    assert_eq!(
        (unsafe { fk_restarts() }, unsafe { fk_live_calls() }),
        (1, 0)
    );
}

#[test]
fn boot_results_follow_the_boot_selection_after_a_live_save() {
    let _g = digikit();
    add("modules/x.dkm", OK | LOADED, 20, 0, 0);
    add("modules/a.dkm", OK, 20, 0, 0);
    select("modules/x.dkm");
    select("modules/a.dkm");
    // the boot loaded x and a failed at selection index 1 for a missing module
    let n = CString::new("usb-bulk").unwrap();
    unsafe { fk_boot_needs(1, n.as_ptr()) };
    open_modules();
    press_yes(0); // unload x: the saved selection is [a] from now on, a has index 0 there and 1 at the boot
    press_no();
    answer_confirm(true);
    poll_until_idle();
    open_modules();
    press_right(1);
    choose_panel_row(1); // INFO
    assert!(
        render().contains(&"NEEDS USB-BULK".to_string()),
        "{:?}",
        render()
    );
}

#[test]
fn the_settings_menu_is_built_once_per_module_whatever_the_applies_do_to_the_module_order() {
    let _g = digikit();
    add("modules/a.dkm", OK | LOADED, 20, 0, 0);
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/b.dkm", OK, 20, 0, 0);
    loaded("modules/a.dkm", "test/a"); // before tool in the runtime's list
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/a.dkm");
    select("modules/tool.dkm");
    open_modules();
    let open_settings = || {
        press_right(1);
        choose_panel_row(1);
        let h = unsafe { fk_set_handle(0) };
        assert_eq!(render(), ["HELD 0", "RELEASE"]);
        press_no();
        h
    };
    let first = open_settings();
    assert_ne!(first, 0);
    // two applies: a is unloaded (tool moves to index 0 of the runtime's list), then b is loaded and a again
    press_yes(0);
    press_no();
    answer_confirm(true);
    poll_until_idle();
    open_modules();
    press_yes(2);
    press_yes(0);
    press_no();
    answer_confirm(true);
    poll_until_idle();
    open_modules();
    assert_eq!(
        open_settings(),
        first,
        "the menu of the module is built once"
    );
    assert_eq!(
        unsafe { fk_settings_runs() },
        2,
        "its rows are filled at each open"
    );
}

#[test]
fn about_names_each_boot_index_as_the_runtime_holds_it() {
    let _g = digikit();
    add("modules/a.dkm", OK | LOADED, 20, 0, 0);
    select("modules/a.dkm");
    unsafe { fk_boot_count(1) };
    let rows = || {
        open_digikit();
        press_yes(0);
        let r = render();
        press_no();
        press_no();
        r
    };
    assert!(rows().iter().any(|s| s == "A.DKM"), "{:?}", rows());
    // a was unloaded and c loaded into its index; d took a new one
    let (c, d) = (
        CString::new("c.dkm").unwrap(),
        CString::new("d.dkm").unwrap(),
    );
    unsafe {
        fk_boot_rename(0, c.as_ptr());
        fk_boot_rename(1, d.as_ptr());
        fk_boot_count(2);
    }
    let r = rows();
    assert!(
        r.iter().any(|s| s == "C.DKM") && r.iter().any(|s| s == "D.DKM"),
        "{r:?}"
    );
    assert!(!r.iter().any(|s| s == "A.DKM"), "{r:?}");
}

#[test]
fn a_failed_unload_and_a_failed_load_leave_marks_in_the_list() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/a.dkm", OK, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_live_script(0, 0, -83) };
    open_modules();
    press_yes(0);
    press_yes(1);
    press_no();
    answer_confirm(true);
    poll_until_idle();
    answer_confirm(false); // NOT APPLIED
    answer_confirm(false); // RESTART TO APPLY?: no
                           // the unload did not happen: tool is loaded and not selected; the load was not tried: a is selected and not loaded
    let r = render();
    assert!(
        r[0].starts_with("  ~TOOL") && r[0].contains("UNLOADS"),
        "{r:?}"
    );
    assert!(r[1].starts_with("  -A"), "{r:?}");
    // the info page says what happens at a restart
    press_right(0);
    choose_panel_row(1);
    assert!(
        render().contains(&"UNLOADS AT RESTART".to_string()),
        "{:?}",
        render()
    );
}

#[test]
fn a_refused_restart_after_the_save_keeps_the_marks_visible() {
    let _g = digikit();
    add("modules/both.dkm", OK | LOADED, 10, 1, 0);
    loaded("modules/both.dkm", "test/both");
    fx("test/both", 3, 1);
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    select("modules/both.dkm");
    select("modules/tool.dkm");
    unsafe { fk_send_slots(2) };
    unsafe { fk_restart_result(-43) };
    open_modules();
    press_yes(1); // unselect tool
    press_right(0);
    choose_panel_row(1);
    pick_choice(0); // and a route change: this restarts directly
    press_no();
    answer_confirm(true);
    assert_eq!(dialog().unwrap().2, "CARD BUSY");
    answer_confirm(false);
    assert!(render()[1].starts_with("  ~TOOL"), "{:?}", render());
}

#[test]
fn a_dialog_that_finds_another_open_waits_for_the_next_pass() {
    let _g = digikit();
    open_digikit();
    unsafe { fk_say_twice() };
    assert_eq!(dialog().unwrap().1, "FIRST");
    answer_confirm(false);
    assert!(dialog().is_none());
    unsafe { fk_say_poll() };
    assert_eq!(dialog().unwrap().1, "SECOND");
}

#[test]
fn the_side_panel_and_its_rows_do_nothing_while_a_batch_runs() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    add("modules/a.dkm", OK, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_live_waits(5) };
    open_modules();
    press_yes(0);
    press_yes(1);
    press_no();
    answer_confirm(true);
    unsafe { fk_poll() };
    let depth_before = depth();
    press_right(0);
    assert_eq!(depth(), depth_before, "no side panel during a batch");
    poll_until_idle();
}

#[test]
fn a_busy_job_slot_reads_waiting() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_live_script(0, -12, 0) }; // E_BUSY at the queue
    open_modules();
    press_yes(0);
    press_no();
    answer_confirm(true);
    unsafe { fk_poll() };
    assert!(render()[0].contains("WAITING"), "{:?}", render());
}

#[test]
fn a_queue_that_cannot_be_polled_shows_the_status_and_offers_the_restart() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    select("modules/tool.dkm");
    unsafe { fk_poll_slots(0) };
    open_modules();
    press_yes(0);
    press_no();
    answer_confirm(true);
    let (_, title, text, _) = dialog().unwrap();
    assert_eq!(
        (title.as_str(), text.as_str()),
        ("NOT APPLIED", "NO MEMORY")
    );
    answer_confirm(false);
    assert_eq!(dialog().unwrap().1, "RESTART TO APPLY?");
}

#[test]
fn a_settings_page_holds_no_module_rows_once_it_is_left() {
    let _g = digikit();
    store_of_three();
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/tool.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert_eq!(
        (unsafe { fk_settings_runs() }, unsafe { fk_set_open(0) }),
        (1, 1)
    );
    // while the page is open nothing is released
    unsafe { fk_settings_poll() };
    assert_eq!(unsafe { fk_set_open(0) }, 1);
    press_no(); // the page is left
    unsafe { fk_settings_poll() };
    // released: the page was filled again without the module (the module's menu function did not run) and nothing keeps it open
    assert_eq!(
        (unsafe { fk_settings_runs() }, unsafe { fk_set_open(0) }),
        (1, 0)
    );
    assert_eq!(unsafe { fk_poll_on() }, 0);
    // opened again, the page works as before
    press_right(0);
    choose_panel_row(1);
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert_eq!(unsafe { fk_settings_runs() }, 2);
    press_yes(1);
    assert_eq!(unsafe { fk_settings_acts() }, 1);
}

/// While a module's SETTINGS page is open the page holds the module with the runtime (`dkrt_menu_built`: its rows point into the module's code, so a live unload or reload is
/// refused with `E_RELOAD_MENUS`); leaving the page gives the hold back (`dkrt_menu_release`), once, and opening it again holds again.
#[test]
fn an_open_settings_page_holds_its_module_and_leaving_it_gives_the_hold_back() {
    let _g = digikit();
    store_of_three();
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/tool.dkm");
    open_modules();
    assert_eq!(unsafe { fk_menu_held() }, 0, "no page open: no hold");
    press_right(0);
    choose_panel_row(1);
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert_eq!(
        unsafe { fk_menu_held() },
        1,
        "the open page holds the module"
    );
    unsafe { fk_settings_poll() };
    assert_eq!(
        unsafe { fk_menu_held() },
        1,
        "polling an open page keeps the hold and takes no second one"
    );
    press_no(); // the page is left
    unsafe { fk_settings_poll() };
    assert_eq!(
        unsafe { fk_menu_held() },
        0,
        "the left page gave the hold back"
    );
    unsafe { fk_settings_poll() };
    assert_eq!(unsafe { fk_menu_held() }, 0, "once");
    press_right(0);
    choose_panel_row(1);
    assert_eq!(unsafe { fk_menu_held() }, 1, "opened again: held again");
}

/// A live apply releases an open page before its jobs (`applying_releases_an_open_settings_page_before_the_jobs`): the hold goes with the page.
#[test]
fn the_release_before_an_apply_gives_the_hold_back_too() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/tool.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    press_no();
    assert_eq!(
        unsafe { fk_menu_held() },
        1,
        "the page is left but not released yet"
    );
    press_yes(0);
    press_no();
    answer_confirm(true);
    assert_eq!(
        unsafe { fk_menu_held() },
        0,
        "the batch that unloads the module finds no hold"
    );
    poll_until_idle();
}

#[test]
fn a_settings_page_of_a_module_that_is_gone_shows_not_loaded_and_calls_nothing() {
    let _g = digikit();
    store_of_three();
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/tool.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    assert_eq!(unsafe { fk_settings_runs() }, 1);
    // the module goes away while its page is filled again
    let path = CString::new("modules/tool.dkm").unwrap();
    unsafe { fk_unload_module(path.as_ptr()) };
    unsafe { fk_set_refresh(0) };
    assert_eq!(render(), ["MODULE NOT LOADED"]);
    assert_eq!(
        unsafe { fk_settings_runs() },
        1,
        "no call reached the old module"
    );
    // loaded again: the page works
    loaded_menu("modules/tool.dkm", "digikit/tool");
    unsafe { fk_set_refresh(0) };
    assert_eq!(render(), ["HELD 0", "RELEASE"]);
    assert_eq!(unsafe { fk_settings_runs() }, 2);
}

#[test]
fn applying_releases_an_open_settings_page_before_the_jobs() {
    let _g = digikit();
    add("modules/tool.dkm", OK | LOADED, 20, 0, 0);
    loaded_menu("modules/tool.dkm", "digikit/tool");
    select("modules/tool.dkm");
    open_modules();
    press_right(0);
    choose_panel_row(1);
    press_no(); // back to the list; the page is left but not released yet
    assert_eq!(unsafe { fk_set_open(0) }, 1);
    press_yes(0);
    press_no();
    answer_confirm(true);
    assert_eq!(
        unsafe { fk_set_open(0) },
        0,
        "no page keeps rows of the module the batch unloads"
    );
    poll_until_idle();
}
