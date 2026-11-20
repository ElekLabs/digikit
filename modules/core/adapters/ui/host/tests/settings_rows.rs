//! The SETTINGS rows registry of core/ui: load order, room for four, removal by owner, rows found by id.
use dkui_host::session;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn sr_reset();
    fn sr_add(owner: usize, text: *const c_char, tag: c_int) -> c_int;
    fn sr_add_no_open(owner: usize, text: *const c_char) -> c_int;
    fn sr_remove(owner: usize) -> u32;
    fn sr_count() -> u32;
    fn sr_text(i: u32) -> *const c_char;
    fn sr_id(i: u32) -> u32;
    fn sr_run(id: u32) -> c_int;
    fn sr_calls(tag: c_int) -> c_int;
}

const E_CONTRACT: c_int = -48;
const E_NOMEM: c_int = -8;

fn add(owner: usize, text: &str, tag: c_int) -> c_int {
    let t = CString::new(text).unwrap();
    unsafe { sr_add(owner, t.as_ptr(), tag) }
}

fn texts() -> Vec<String> {
    (0..unsafe { sr_count() })
        .map(|i| {
            unsafe { CStr::from_ptr(sr_text(i)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn rows_keep_load_order_and_take_four() {
    let _g = session();
    unsafe { sr_reset() };
    for (i, t) in ["A", "B", "C", "D"].iter().enumerate() {
        assert_eq!(add(1, t, i as c_int), 0);
    }
    assert_eq!(add(1, "E", 4), E_NOMEM);
    assert_eq!(texts(), ["A", "B", "C", "D"]);
}

#[test]
fn a_row_needs_text_and_an_open_function() {
    let _g = session();
    unsafe { sr_reset() };
    assert_eq!(unsafe { sr_add_no_open(1, c"X".as_ptr()) }, E_CONTRACT);
    assert_eq!(unsafe { sr_add(1, std::ptr::null(), 0) }, E_CONTRACT);
    assert_eq!(unsafe { sr_count() }, 0);
}

#[test]
fn long_text_is_cut_to_the_row_size() {
    let _g = session();
    unsafe { sr_reset() };
    assert_eq!(add(1, &"X".repeat(40), 0), 0);
    assert_eq!(texts()[0].len(), dkui_host::DK_UI_ROW - 1);
}

#[test]
fn removing_an_owner_keeps_the_order_of_the_others_and_frees_room() {
    let _g = session();
    unsafe { sr_reset() };
    add(1, "A", 0);
    add(2, "B", 1);
    add(1, "C", 2);
    add(3, "D", 3);
    assert_eq!(unsafe { sr_remove(1) }, 2);
    assert_eq!(texts(), ["B", "D"]);
    assert_eq!(unsafe { sr_remove(1) }, 0);
    assert_eq!(add(4, "E", 4), 0);
    assert_eq!(add(4, "F", 5), 0);
    assert_eq!(texts(), ["B", "D", "E", "F"]);
}

#[test]
fn a_row_runs_by_id_even_after_the_rows_before_it_moved() {
    let _g = session();
    unsafe { sr_reset() };
    add(1, "A", 0);
    add(2, "B", 1);
    let id_b = unsafe { sr_id(1) };
    unsafe { sr_remove(1) };
    // B is now row 0; the stock row built before still holds B's id
    assert_eq!(unsafe { sr_run(id_b) }, 1);
    assert_eq!((unsafe { sr_calls(0) }, unsafe { sr_calls(1) }), (0, 1));
}

#[test]
fn a_removed_row_does_nothing_when_its_old_stock_row_runs() {
    let _g = session();
    unsafe { sr_reset() };
    add(1, "A", 0);
    let id = unsafe { sr_id(0) };
    unsafe { sr_remove(1) };
    assert_eq!(unsafe { sr_run(id) }, 0);
    assert_eq!(unsafe { sr_calls(0) }, 0);
    // a new row never takes the old id
    add(1, "A", 0);
    assert_ne!(unsafe { sr_id(0) }, id);
}
