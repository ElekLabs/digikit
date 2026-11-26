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
    fn sr_owner_name(handle: usize, out: *mut c_char) -> c_int;
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

/// A service file is an owner like a module: two services each get a row of their own, keyed by their own names, and the unload of one takes only its row.
#[test]
fn two_services_keep_their_own_keys_and_an_unload_takes_only_its_own_row() {
    let _g = session();
    unsafe { sr_reset() };
    // the handles of two service records (owner id + 1, after the content modules' records)
    let (a, b) = (41, 42);
    assert_eq!(add_of(a, "services/a", "A", 0), 0);
    assert_eq!(add_of(b, "services/b", "B", 1), 0);
    let (key_a, key_b) = (unsafe { sr_key(0) }, unsafe { sr_key(1) });
    assert_ne!(key_a, key_b, "two services never share a row key");
    assert_eq!(unsafe { sr_remove(a) }, 1);
    assert_eq!(texts(), ["B"]);
    assert_eq!(
        unsafe { sr_run(key_a) },
        0,
        "the stock row of the unloaded service does nothing"
    );
    assert_eq!(unsafe { sr_run(key_b) }, 1);
    assert_eq!((unsafe { sr_calls(0) }, unsafe { sr_calls(1) }), (0, 1));
}

/// An owner whose name could not be found gets no row: two such owners would otherwise share the key of the empty name and one would run the other's row.
#[test]
fn an_owner_without_a_name_gets_no_row() {
    let _g = session();
    unsafe { sr_reset() };
    assert_eq!(add_of(41, "", "A", 0), E_CONTRACT);
    assert_eq!(unsafe { sr_count() }, 0);
    assert_eq!(
        unsafe { sr_add(41, std::ptr::null(), c"A".as_ptr(), 0) },
        E_CONTRACT
    );
    assert_eq!(unsafe { sr_count() }, 0);
}

/// The row name of an owner handle (record index + 1): the record behind it, a name of 31 characters whole, no row for the core handle, an unloaded record or a name of 32 bytes.
#[test]
fn owner_name_follows_the_handle_to_its_record() {
    let _g = session();
    let name_of = |handle: usize| {
        let mut out = [0x7f as c_char; 32];
        let ok = unsafe { sr_owner_name(handle, out.as_mut_ptr()) };
        (
            ok,
            unsafe { CStr::from_ptr(out.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    };
    assert_eq!(
        name_of(3),
        (1, "svc.two".to_string()),
        "handle 3 is record 2"
    );
    assert_eq!(
        name_of(4),
        (1, "a".repeat(31)),
        "31 characters are kept whole"
    );
    assert_eq!(name_of(2), (0, String::new()), "record 1 is not loaded");
    assert_eq!(
        name_of(0),
        (0, String::new()),
        "the core handle has no record"
    );
    assert_eq!(
        name_of(5),
        (0, String::new()),
        "a name of 32 bytes could share a key"
    );
}

mod rebuild {
    use super::*;
    extern "C" {
        fn sr_built(with_menu: c_int);
        fn sr_pass() -> c_int;
        fn sr_rebuilds() -> c_int;
        fn sr_dirty() -> c_int;
    }

    #[test]
    fn a_row_added_before_the_first_build_needs_no_rebuild() {
        let _g = session();
        unsafe { sr_reset() };
        add(1, "A", 0);
        assert_eq!(unsafe { sr_dirty() }, 0);
        assert_eq!(unsafe { sr_pass() }, 0);
        assert_eq!(unsafe { sr_rebuilds() }, 0);
    }

    #[test]
    fn a_row_added_after_the_build_makes_one_rebuild_on_the_next_pass() {
        let _g = session();
        unsafe { sr_reset() };
        unsafe { sr_built(1) };
        add(1, "A", 0);
        assert_eq!(unsafe { sr_dirty() }, 1);
        assert_eq!(unsafe { sr_pass() }, 1);
        assert_eq!(unsafe { sr_pass() }, 0, "the rebuild cleared the mark");
        assert_eq!(unsafe { sr_rebuilds() }, 1);
    }

    #[test]
    fn removing_a_row_at_unload_rebuilds_but_removing_nothing_does_not() {
        let _g = session();
        unsafe { sr_reset() };
        add(1, "A", 0);
        unsafe { sr_built(1) };
        assert_eq!(unsafe { sr_remove(2) }, 0, "an owner without rows");
        assert_eq!(unsafe { sr_pass() }, 0);
        assert_eq!(unsafe { sr_remove(1) }, 1);
        assert_eq!(unsafe { sr_pass() }, 1);
        assert_eq!(unsafe { sr_rebuilds() }, 1);
    }

    #[test]
    fn with_no_list_nothing_is_called() {
        let _g = session();
        unsafe { sr_reset() };
        unsafe { sr_built(0) };
        add(1, "A", 0);
        unsafe { sr_remove(1) };
        assert_eq!(unsafe { sr_pass() }, 0);
        assert_eq!(unsafe { sr_rebuilds() }, 0);
    }
}
