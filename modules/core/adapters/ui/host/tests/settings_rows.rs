//! The SETTINGS rows registry of core/ui: load order, room for four, removal by owner, rows found by id.
use dkui_host::session;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn sr_reset();
    fn sr_add(owner: usize, name: *const c_char, text: *const c_char, tag: c_int) -> c_int;
    fn sr_add_outside_init(owner: usize, text: *const c_char) -> c_int;
    fn sr_add_no_open(owner: usize, text: *const c_char) -> c_int;
    fn sr_remove(owner: usize) -> u32;
    fn sr_count() -> u32;
    fn sr_text(i: u32) -> *const c_char;
    fn sr_key(i: u32) -> usize;
    fn sr_run(key: usize) -> c_int;
    fn sr_calls(tag: c_int) -> c_int;
}

const E_CONTRACT: c_int = -48;
const E_CONTEXT: c_int = -19;
const E_NOMEM: c_int = -8;

/// A row of the module `name` (the owner is its record).
fn add_of(owner: usize, name: &str, text: &str, tag: c_int) -> c_int {
    let (n, t) = (CString::new(name).unwrap(), CString::new(text).unwrap());
    unsafe { sr_add(owner, n.as_ptr(), t.as_ptr(), tag) }
}
fn add(owner: usize, text: &str, tag: c_int) -> c_int {
    add_of(owner, &format!("m{owner}"), text, tag)
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
    assert_eq!(
        unsafe { sr_add(1, c"m".as_ptr(), std::ptr::null(), 0) },
        E_CONTRACT
    );
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
fn a_stock_row_runs_its_row_even_after_the_rows_before_it_moved() {
    let _g = session();
    unsafe { sr_reset() };
    add(1, "A", 0);
    add(2, "B", 1);
    let key_b = unsafe { sr_key(1) };
    unsafe { sr_remove(1) };
    // B is now row 0; the stock row built before still holds B's key
    assert_eq!(unsafe { sr_run(key_b) }, 1);
    assert_eq!((unsafe { sr_calls(0) }, unsafe { sr_calls(1) }), (0, 1));
}

#[test]
fn after_an_unload_the_old_stock_row_does_nothing() {
    let _g = session();
    unsafe { sr_reset() };
    add(1, "A", 0);
    let key = unsafe { sr_key(0) };
    unsafe { sr_remove(1) };
    assert_eq!(unsafe { sr_run(key) }, 0);
    assert_eq!(unsafe { sr_calls(0) }, 0);
}

#[test]
fn after_a_reload_the_old_stock_row_runs_the_reloaded_modules_row() {
    let _g = session();
    unsafe { sr_reset() };
    add_of(1, "digikit/demo", "A", 0);
    let key = unsafe { sr_key(0) };
    unsafe { sr_remove(1) };
    // the same module loads again into its record and adds its row again, with other callback data
    add_of(1, "digikit/demo", "A", 3);
    assert_eq!(
        unsafe { sr_key(0) },
        key,
        "the same module and the same row number: the same key"
    );
    assert_eq!(unsafe { sr_run(key) }, 1);
    assert_eq!(
        (unsafe { sr_calls(0) }, unsafe { sr_calls(3) }),
        (0, 1),
        "the new registration ran"
    );
}

#[test]
fn two_modules_with_rows_keep_their_own_and_a_module_numbers_its_rows() {
    let _g = session();
    unsafe { sr_reset() };
    add_of(1, "a/one", "A1", 0);
    add_of(1, "a/one", "A2", 1);
    add_of(2, "b/two", "B1", 2);
    let keys: Vec<usize> = (0..3).map(|i| unsafe { sr_key(i) }).collect();
    assert!(
        keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2],
        "three keys"
    );
    // module b alone unloads and reloads: only its row changes registration
    unsafe { sr_remove(2) };
    add_of(2, "b/two", "B1", 4);
    for (k, tag) in [(keys[0], 0), (keys[1], 1), (keys[2], 4)] {
        assert_eq!(unsafe { sr_run(k) }, 1);
        assert_eq!(unsafe { sr_calls(tag) }, 1, "key of tag {tag}");
    }
    assert_eq!(
        unsafe { sr_calls(2) },
        0,
        "the old registration of b never ran"
    );
}

#[test]
fn the_keys_run_out_after_many_distinct_rows_over_time() {
    let _g = session();
    unsafe { sr_reset() };
    // eight keys in all: modules that come and go use them up
    for i in 0..8 {
        assert_eq!(add_of(1, &format!("mod{i}"), "R", 0), 0, "{i}");
        unsafe { sr_remove(1) };
    }
    assert_eq!(add_of(1, "one-more", "R", 0), E_NOMEM);
}

#[test]
fn a_row_added_outside_init_is_refused_with_the_context_error() {
    let _g = session();
    unsafe { sr_reset() };
    assert_eq!(unsafe { sr_add_outside_init(1, c"A".as_ptr()) }, E_CONTEXT);
    assert_eq!(unsafe { sr_count() }, 0);
    assert_eq!(add(1, "A", 0), 0, "init still works");
}
