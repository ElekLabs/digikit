//! The DIGIKIT menu as a mechanism, on the fake runtime table: the structure comes from the data the table gives (one row per scanned module, the saved selection read with
//! `dkrt_selection_get` and written with `dkrt_selection_set`, the health and status pages read the table), the SETTINGS rows come from the one hook function, and empty or degraded
//! data leave a working menu. No row text and no item name is pinned here: `menu.rs` owns what the screens say.
use dkui_host::driver::*;
use dkui_host::*;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn fk_reset();
    fn fk_add(path: *const c_char, flags: u32, arena: u32, fx_slots: u32, machines: u32);
    fn fk_select(path: *const c_char);
    fn fk_scan_result(r: c_int);
    fn fk_selection_get_result(r: c_int);
    fn fk_boot_safe();
    fn fk_open_digikit() -> c_int;
    fn fk_selection_sets() -> c_int;
    fn fk_set_count() -> c_int;
    fn fk_set_path(i: c_int) -> *const c_char;
    fn fk_tick(i: c_int) -> c_int;
    fn fk_stock_rows() -> c_int;
    fn fk_health_calls() -> c_int;
    fn fk_status_calls() -> c_int;
}

/// dkrt_scan flags: this runtime and device can load it, and its needs are met.
const OK: u32 = 3;
const KB: u32 = 1024;
/// Where the pages are in the DIGIKIT menu: the order of the main menu's rows, ABOUT, HEALTH, MEMORY, MODULES.
const ABOUT: usize = 0;
const HEALTH: usize = 1;
const MODULES: usize = 3;

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe { fk_reset() };
    g
}

/// `n` modules the scan reports, `modules/m0.dkm` ...
fn store(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let path = format!("modules/m{i}.dkm");
            let p = CString::new(path.as_str()).unwrap();
            unsafe { fk_add(p.as_ptr(), OK, (10 + i as u32) * KB, 0, 0) };
            path
        })
        .collect()
}

fn select(path: &str) {
    let p = CString::new(path).unwrap();
    unsafe { fk_select(p.as_ptr()) }
}

fn open_page(row: usize) {
    assert_eq!(
        unsafe { fk_open_digikit() },
        0,
        "the stock row opens a menu"
    );
    press_yes(row);
    assert_eq!(depth(), 2, "the page opened");
}

fn ticked(n: usize) -> Vec<usize> {
    (0..n)
        .filter(|&i| unsafe { fk_tick(i as c_int) } == 1)
        .collect()
}

fn written() -> Vec<String> {
    (0..unsafe { fk_set_count() })
        .map(|i| {
            unsafe { CStr::from_ptr(fk_set_path(i)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// The MODULES page has one row per module the scan reports, for any count; it has no module row with nothing to list, and still opens.
#[test]
fn the_modules_page_has_one_row_per_scanned_module() {
    for n in [1, 2, 5, 9] {
        let _g = fresh();
        store(n);
        open_page(MODULES);
        let rows = labels();
        assert_eq!(rows.len(), n, "{n} modules");
        let mut distinct = rows.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), n, "each module has its own row");
    }
    let _g = fresh();
    open_page(MODULES);
    assert!(
        labels().len() <= 1,
        "no module rows for an empty store: {:?}",
        labels()
    );
    assert_eq!(ticked(4), Vec::<usize>::new());
}

/// The check marks start from the selection that `dkrt_selection_get` returns, and an apply writes the marked paths with `dkrt_selection_set`: the selection is kept in the
/// table's record, not in the menu. Leaving with nothing changed asks nothing and writes nothing.
#[test]
fn the_saved_selection_is_read_from_the_table_and_the_marked_set_is_written_back() {
    let _g = fresh();
    let paths = store(5);
    select(&paths[1]);
    select(&paths[3]);
    open_page(MODULES);
    assert_eq!(
        ticked(5),
        vec![1, 3],
        "the marks start from the saved selection"
    );
    press_no();
    assert!(dialog().is_none(), "nothing changed: no question");
    assert_eq!(
        unsafe { fk_selection_sets() },
        0,
        "nothing changed: nothing written"
    );

    open_page(MODULES);
    press_yes(0);
    press_yes(3);
    assert_eq!(
        ticked(5),
        vec![0, 1],
        "row 3 was marked, then unmarked by its own toggle"
    );
    press_no();
    assert!(
        dialog().is_some(),
        "a change is asked about before it is applied"
    );
    answer_confirm(true);
    assert_eq!(unsafe { fk_selection_sets() }, 1);
    let mut got = written();
    got.sort();
    assert_eq!(
        got,
        vec![paths[0].clone(), paths[1].clone()],
        "the written set is the marked rows"
    );
}

/// A selection or a scan the table cannot give lists no module, marks nothing, and leaves a menu that goes back and works again once the table answers.
#[test]
fn a_selection_or_a_scan_the_table_refuses_lists_no_module_and_the_menu_recovers() {
    let _g = fresh();
    let paths = store(3);
    select(&paths[0]);
    unsafe { fk_selection_get_result(-1) };
    open_page(MODULES);
    assert!(
        labels().len() <= 1,
        "an unreadable selection lists no module: {:?}",
        labels()
    );
    assert_eq!(
        ticked(3),
        Vec::<usize>::new(),
        "an unreadable selection marks nothing"
    );
    press_no();
    press_no();
    unsafe { fk_selection_get_result(0) };
    open_page(MODULES);
    assert_eq!(labels().len(), 3, "the table answers again");
    press_no();
    press_no();
    unsafe { fk_scan_result(-1) };
    open_page(MODULES);
    assert!(
        labels().len() <= 1,
        "a failed scan lists no module: {:?}",
        labels()
    );
}

/// The SETTINGS row is added by the one hook function, once, and opens the menu; a safe boot (the table says so in the boot info) shows the menu with one row more.
#[test]
fn the_hook_adds_one_settings_row_and_a_safe_boot_adds_a_page_row() {
    let _g = fresh();
    assert_eq!(unsafe { fk_stock_rows() }, 0);
    assert_eq!(unsafe { fk_open_digikit() }, 0);
    assert_eq!(unsafe { fk_stock_rows() }, 1, "one row in the stock menu");
    let normal = render().len();
    unsafe { fk_reset() };
    unsafe { fk_boot_safe() };
    assert_eq!(unsafe { fk_open_digikit() }, 0);
    assert_eq!(render().len(), normal + 1, "safe mode adds its row");
}

/// The health and status figures are read from the table when their pages are opened, not before.
#[test]
fn the_health_and_about_pages_read_the_table_when_opened() {
    let _g = fresh();
    store(2);
    assert_eq!(unsafe { fk_open_digikit() }, 0);
    assert_eq!(
        (unsafe { fk_health_calls() }, unsafe { fk_status_calls() }),
        (0, 0)
    );
    press_yes(HEALTH);
    assert!(
        unsafe { fk_health_calls() } > 0,
        "the health page asks dkrt_health"
    );
    press_no();
    assert_eq!(
        unsafe { fk_status_calls() },
        0,
        "the status is not read for another page"
    );
    press_yes(ABOUT);
    assert!(
        unsafe { fk_status_calls() } > 0,
        "the about page asks dkrt_status"
    );
}
