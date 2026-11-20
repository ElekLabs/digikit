//! The desc checks of dk_ui_text_input: the length rules, the null checks and the start text.
use dkui_host::session;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn ti_check(what: c_int, max_len: u32, screen_max: u32) -> i32;
    fn ti_initial(initial: *const c_char, has_initial: c_int, max_len: u32, out: *mut c_char);
}

const E_CONTRACT: i32 = -48;

fn check(what: i32, max_len: u32, screen_max: u32) -> i32 {
    let _g = session();
    unsafe { ti_check(what, max_len, screen_max) }
}

fn initial(text: Option<&str>, max_len: u32) -> String {
    let _g = session();
    let c = CString::new(text.unwrap_or("")).unwrap();
    let mut out = [0 as c_char; 16];
    unsafe {
        ti_initial(
            c.as_ptr(),
            text.is_some() as c_int,
            max_len,
            out.as_mut_ptr(),
        )
    };
    unsafe { CStr::from_ptr(out.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_length_from_one_to_fifteen_that_fits_the_screen_is_accepted() {
    assert_eq!(check(0, 1, 15), 0);
    assert_eq!(check(0, 15, 15), 0);
    assert_eq!(check(0, 15, 40), 0);
}

#[test]
fn a_length_of_zero_or_above_fifteen_is_refused() {
    assert_eq!(check(0, 0, 15), E_CONTRACT);
    assert_eq!(check(0, 16, 40), E_CONTRACT);
}

#[test]
fn a_length_above_what_the_screen_takes_is_refused() {
    assert_eq!(check(0, 12, 11), E_CONTRACT);
    assert_eq!(check(0, 11, 11), 0);
    assert_eq!(check(0, 1, 0), E_CONTRACT);
}

#[test]
fn a_desc_needs_done_and_a_title() {
    assert_eq!(check(1, 8, 15), E_CONTRACT, "no desc");
    assert_eq!(check(2, 8, 15), E_CONTRACT, "no done");
    assert_eq!(check(3, 8, 15), E_CONTRACT, "no title");
}

#[test]
fn the_start_text_is_cut_to_the_length_and_empty_without_text() {
    assert_eq!(initial(Some("KICK"), 8), "KICK");
    assert_eq!(initial(Some("BASS DRUM LONG"), 4), "BASS");
    assert_eq!(initial(Some(""), 4), "");
    assert_eq!(initial(None, 4), "");
}
