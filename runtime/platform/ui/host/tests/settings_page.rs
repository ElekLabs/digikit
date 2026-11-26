//! The settings page (sdk/src/settings/page.c) on the fake <dk/platform/ui.h> and a fake settings store: the rows, the saves, the popup of an enum, a refused
//! save, a restart field and the checks of a field table. The text of a value (field.c) has its case at the end.
use dkui_host::driver::*;
use dkui_host::*;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn sp_reset();
    fn sp_store(key: *const c_char, text: *const c_char);
    fn sp_stored(key: *const c_char) -> *const c_char;
    fn sp_init() -> c_int;
    fn sp_init_without(settings: c_int, ui: c_int) -> c_int;
    fn sp_init_bool_only() -> c_int;
    fn sp_page() -> *mut DkUiMenu;
    fn sp_check_case(n: c_int) -> c_int;
    fn sp_changed_calls() -> c_int;
    fn sp_changed_field() -> c_int;
    fn sp_changed_value() -> c_int;
    fn sp_puts_at_changed() -> c_int;
    fn sp_puts() -> c_int;
    fn sp_fills() -> c_int;
    fn sp_fills_at_changed() -> c_int;
    fn sp_put_result(rc: c_int);
    fn sp_value(field: c_int) -> c_int;
    fn sp_set(field: c_int, value: c_int) -> c_int;

    fn dk_settings_field_encode(value: i32, out: *mut c_char) -> u32;
}

const E_FORMAT: i32 = -5;
const E_NOT_FOUND: i32 = -1;
const E_MISSING: i32 = -73;
const E_SET_FULL: i32 = -67;

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn stored(key: &str) -> Option<String> {
    let p = unsafe { sp_stored(c(key).as_ptr()) };
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string())
}

fn store(key: &str, text: &str) {
    unsafe { sp_store(c(key).as_ptr(), c(text).as_ptr()) }
}

/// A fresh page over an empty store; `prepare` runs before init (stored values).
fn page_with(prepare: impl FnOnce()) -> (std::sync::MutexGuard<'static, ()>, *mut DkUiMenu) {
    let g = session();
    unsafe { sp_reset() };
    prepare();
    assert_eq!(unsafe { sp_init() }, 0);
    let p = unsafe { sp_page() };
    assert_eq!(open(p), 0);
    (g, p)
}

fn page() -> (std::sync::MutexGuard<'static, ()>, *mut DkUiMenu) {
    page_with(|| {})
}

/// The rows as drawn, with every run of spaces (the padding of a column) as one space.
fn rows() -> Vec<String> {
    render()
        .iter()
        .map(|r| r.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

const DEFAULTS: [&str; 5] = [
    "LOUD ON",
    "GAIN 050",
    "TRIM -05",
    "MODE SLOW",
    "BOOT FAST OFF",
];

#[test]
fn a_new_page_shows_the_defaults_in_the_wide_page() {
    let (_g, _p) = page();
    assert!(is_wide());
    assert_eq!(title(), "TEST");
    assert_eq!(rows(), DEFAULTS);
    assert_eq!(unsafe { sp_puts() }, 0, "showing the page writes nothing");
}

#[test]
fn stored_values_are_read_in_init_and_a_bad_one_reads_as_the_default() {
    let (_g, _p) = page_with(|| {
        store("loud", "0");
        store("gain", "7");
        store("trim", "99"); // out of -10 .. 10
        store("mode", "x"); // not a number
        store("boot", "1");
    });
    assert_eq!(
        rows(),
        [
            "LOUD OFF",
            "GAIN 007",
            "TRIM -05",
            "MODE SLOW",
            "BOOT FAST ON"
        ]
    );
    assert_eq!(unsafe { sp_value(1) }, 7);
    assert_eq!(unsafe { sp_value(9) }, 0, "no such field");
}

#[test]
fn a_toggle_is_saved_once_before_changed_runs() {
    let (_g, _p) = page();
    let fills = unsafe { sp_fills() };
    press_yes(0);
    assert!(
        unsafe { sp_fills_at_changed() } > fills,
        "the wide page is filled again before changed runs, as core/ui does"
    );
    assert_eq!(rows()[0], "LOUD OFF");
    assert_eq!(stored("loud").as_deref(), Some("0"));
    assert_eq!(
        unsafe { sp_puts() },
        1,
        "the refill before changed saves nothing"
    );
    assert_eq!(unsafe { sp_changed_calls() }, 1);
    assert_eq!(
        (unsafe { sp_changed_field() }, unsafe { sp_changed_value() }),
        (0, 0)
    );
    assert_eq!(
        unsafe { sp_puts_at_changed() },
        1,
        "the value is saved when changed runs"
    );
    press_yes(0);
    assert_eq!(rows()[0], "LOUD ON");
    assert_eq!(stored("loud").as_deref(), Some("1"));
}

#[test]
fn a_value_row_turns_inside_its_range_and_saves_decimal_text() {
    let (_g, _p) = page();
    turn(1, 7);
    assert_eq!(rows()[1], "GAIN 057");
    assert_eq!(stored("gain").as_deref(), Some("57"));
    turn(1, 100);
    assert_eq!(stored("gain").as_deref(), Some("100"));
    let calls = unsafe { sp_changed_calls() };
    turn(1, 5); // already at the top: no change, no save
    assert_eq!(unsafe { sp_changed_calls() }, calls);
    turn(2, -100);
    assert_eq!(rows()[2], "TRIM -10");
    assert_eq!(stored("trim").as_deref(), Some("-10"));
    assert_eq!(
        (unsafe { sp_changed_field() }, unsafe { sp_changed_value() }),
        (2, -10)
    );
}

#[test]
fn an_enum_opens_the_choice_and_a_pick_saves_and_shows_the_option() {
    let (_g, _p) = page();
    press_yes(3);
    let (kind, title, _, items) = dialog().expect("the popup");
    assert_eq!(kind, DialogKind::Choice);
    assert_eq!(title, "MODE");
    assert_eq!(items, ["OFF", "SLOW", "FAST"]);
    pick_choice(2);
    assert_eq!(rows()[3], "MODE FAST");
    assert_eq!(stored("mode").as_deref(), Some("2"));
    assert_eq!(
        (unsafe { sp_changed_field() }, unsafe { sp_changed_value() }),
        (3, 2)
    );
    assert_eq!(unsafe { sp_changed_calls() }, 1);
    // NO on the popup changes nothing
    press_yes(3);
    press_no();
    assert_eq!(rows()[3], "MODE FAST");
    assert_eq!(unsafe { sp_changed_calls() }, 1);
}

#[test]
fn a_refused_save_puts_the_old_value_back_and_says_so_until_a_save_works() {
    let (_g, _p) = page();
    unsafe { sp_put_result(E_SET_FULL) };
    press_yes(0);
    assert_eq!(unsafe { sp_value(0) }, 1);
    let mut want = DEFAULTS.map(String::from).to_vec();
    want.push("NOT SAVED".into());
    assert_eq!(rows(), want);
    assert_eq!(
        unsafe { sp_changed_calls() },
        0,
        "changed runs after a save only"
    );
    unsafe { sp_put_result(0) };
    press_yes(0);
    assert_eq!(rows()[0], "LOUD OFF");
    assert_eq!(rows().len(), 5, "NOT SAVED is gone");
}

#[test]
fn a_restart_field_adds_its_note_after_a_change() {
    let (_g, _p) = page();
    press_yes(4);
    let got = rows();
    assert_eq!(got[4], "BOOT FAST ON");
    assert_eq!(got[5], "ON NEXT RESTART");
    assert_eq!(unsafe { sp_changed_calls() }, 1);
}

#[test]
fn set_saves_and_shows_but_does_not_call_changed() {
    let (_g, _p) = page();
    assert_eq!(unsafe { sp_set(1, 150) }, 0);
    assert_eq!(rows()[1], "GAIN 100", "clamped");
    assert_eq!(stored("gain").as_deref(), Some("100"));
    assert_eq!(unsafe { sp_changed_calls() }, 0);
    assert_eq!(unsafe { sp_set(7, 1) }, E_NOT_FOUND);
    unsafe { sp_put_result(E_SET_FULL) };
    assert_eq!(unsafe { sp_set(1, 20) }, E_SET_FULL);
    assert_eq!(unsafe { sp_value(1) }, 100, "the value stays as it was");
}

#[test]
fn a_page_that_was_not_initialised_shows_one_text_row() {
    let _g = session();
    unsafe { sp_reset() };
    let p = unsafe { sp_page() };
    assert_eq!(open(p), 0);
    assert_eq!(rows(), ["NO SETTINGS"]);
}

#[test]
fn init_needs_the_handles_and_a_ui_only_for_an_enum() {
    let _g = session();
    unsafe { sp_reset() };
    assert_eq!(unsafe { sp_init_without(0, 1) }, E_MISSING);
    assert_eq!(
        unsafe { sp_init_without(1, 0) },
        E_MISSING,
        "the table has an enum"
    );
    assert_eq!(unsafe { sp_init_bool_only() }, 0);
    assert_eq!(unsafe { sp_init_without(1, 1) }, 0);
}

#[test]
fn a_bad_table_is_refused() {
    let _g = session();
    assert_eq!(unsafe { sp_check_case(0) }, 0, "the good table");
    // no count, too many, no table, empty key, "/", "..", a repeated key, a BOOL default of 2, an INT range past 999, an INT default outside,
    // an ENUM without labels or with one option, a row too wide, an unknown kind
    for n in 1..=14 {
        assert_eq!(unsafe { sp_check_case(n) }, E_FORMAT, "case {n}");
    }
}

#[test]
fn values_are_decimal_text_of_at_most_four_characters() {
    let enc = |v: i32| {
        let mut out = [0 as c_char; 8];
        let n = unsafe { dk_settings_field_encode(v, out.as_mut_ptr()) } as usize;
        let s = unsafe { CStr::from_ptr(out.as_ptr()) }
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(s.len(), n);
        s
    };
    assert_eq!(enc(0), "0");
    assert_eq!(enc(7), "7");
    assert_eq!(enc(-99), "-99");
    assert_eq!(enc(999), "999");
}
