//! The desc checks of dk_ui_text_input: the length rules, the null checks and the start text.
use dkui_host::session;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn ti_check(what: c_int, max_len: u32, screen_max: u32) -> i32;
    fn ti_open_in_key(owner: usize, tag: c_int, max_len: u32, code: u32) -> i32;
    fn ti_release(code: u32);
    fn ti_defer_poll();
    fn ti_defer_pending() -> u32;
    fn ti_reset();
    fn ti_events(kind: c_int) -> c_int;
    fn ti_set_push_result(r: c_int);
    fn ti_open(owner: usize, tag: c_int, max_len: u32) -> i32;
    fn ti_yes(text: *const c_char);
    fn ti_no();
    fn ti_gone_serial(serial: u32);
    fn ti_drop(owner: usize);
    fn ti_pushed_serial() -> u32;
    fn ti_pushes() -> c_int;
    fn ti_done_calls(tag: c_int) -> c_int;
    fn ti_done_text(tag: c_int) -> *const c_char;
    fn ti_pushed_initial() -> *const c_char;
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

const E_BUSY: i32 = -12;
const E_NOMEM: i32 = -8;

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe { ti_reset() };
    g
}

fn yes(text: &str) {
    let t = CString::new(text).unwrap();
    unsafe { ti_yes(t.as_ptr()) }
}

fn text_of(tag: c_int) -> String {
    unsafe { CStr::from_ptr(ti_done_text(tag)) }
        .to_string_lossy()
        .into_owned()
}

#[test]
fn an_input_opens_the_screen_with_the_start_text_and_yes_calls_done_with_the_text() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 8), 0);
        assert_eq!(
            CStr::from_ptr(ti_pushed_initial()).to_str().unwrap(),
            "KICK"
        );
        yes("BASS");
        assert_eq!((ti_done_calls(0), text_of(0)), (1, "BASS".to_string()));
    }
}

#[test]
fn no_calls_nothing_and_frees_the_input() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 8), 0);
        ti_no();
        assert_eq!(ti_events(1), 0, "NO is no TEXT_INPUT_DONE");
        assert_eq!(ti_done_calls(0), 0);
        assert_eq!(ti_open(1, 1, 8), 0, "free again after NO");
    }
}

#[test]
fn a_second_input_is_refused_while_one_is_on_the_screen() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 8), 0);
        assert_eq!(ti_open(2, 1, 8), E_BUSY);
        assert_eq!(
            ti_pushes(),
            1,
            "the screen was not built for the refused one"
        );
        yes("A");
        assert_eq!(ti_open(2, 1, 8), 0);
    }
}

#[test]
fn a_wrong_desc_is_refused_before_anything_is_taken() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 0), E_CONTRACT);
        assert_eq!(ti_open(1, 0, 17), E_CONTRACT);
        assert_eq!(ti_pushes(), 0);
        assert_eq!(ti_open(1, 0, 8), 0, "the input is not stuck");
    }
}

#[test]
fn a_screen_the_firmware_could_not_build_leaves_the_input_free() {
    let _g = fresh();
    unsafe {
        ti_set_push_result(E_NOMEM);
        assert_eq!(ti_open(1, 0, 8), E_NOMEM);
        ti_set_push_result(0);
        assert_eq!(ti_open(1, 0, 8), 0);
        yes("X");
        assert_eq!(ti_done_calls(0), 1);
    }
}

#[test]
fn done_may_open_the_next_input() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 8), 0);
        let first = ti_pushed_serial();
        yes("A");
        assert_eq!(ti_open(1, 1, 8), 0);
        assert_ne!(ti_pushed_serial(), first);
        // the old screen's detach comes after done: it must not end the new input
        ti_gone_serial(first);
        assert_eq!(ti_open(2, 2, 8), E_BUSY, "the second input is still open");
    }
}

#[test]
fn when_the_owner_unloads_with_the_input_open_yes_calls_nothing_and_the_screen_still_frees_it() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open(1, 0, 8), 0);
        ti_drop(2); // another owner: nothing changes
        ti_drop(1);
        assert_eq!(ti_open(3, 1, 8), E_BUSY, "the screen is still there");
        yes("A");
        assert_eq!(ti_done_calls(0), 0, "the code of the module is gone");
        assert_eq!(
            ti_open(3, 1, 8),
            0,
            "a second module's input works afterwards"
        );
        yes("B");
        assert_eq!((ti_done_calls(1), text_of(1)), (1, "B".to_string()));
    }
}

#[test]
fn dropping_an_owner_without_an_open_input_does_nothing() {
    let _g = fresh();
    unsafe {
        ti_drop(1);
        assert_eq!(ti_open(1, 0, 8), 0);
        yes("A");
        assert_eq!(ti_done_calls(0), 1);
    }
}

#[test]
fn an_input_opened_in_a_key_press_is_pushed_after_the_release() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open_in_key(1, 0, 8, 10), 0);
        assert_eq!(
            ti_pushes(),
            0,
            "the screen would take the release of the key as its YES"
        );
        assert_eq!(ti_open(2, 1, 8), E_BUSY, "but the input is taken");
        ti_release(10);
        assert_eq!(ti_pushes(), 0);
        assert_eq!(ti_events(0), 0, "no event before the screen is pushed");
        ti_defer_poll();
        assert_eq!(ti_pushes(), 1);
        assert_eq!(ti_events(0), 1, "TEXT_INPUT_OPENED when it was pushed");
        assert_eq!(
            CStr::from_ptr(ti_pushed_initial()).to_str().unwrap(),
            "KICK"
        );
        assert_eq!(ti_events(1), 0);
        yes("OK");
        assert_eq!(ti_events(1), 1, "TEXT_INPUT_DONE on YES");
        assert_eq!(ti_events(2), 0, "both events carry the keep flag");
        assert_eq!((ti_done_calls(0), text_of(0)), (1, "OK".to_string()));
    }
}

#[test]
fn the_desc_may_be_gone_before_the_deferred_push() {
    let _g = fresh();
    // ti_open builds its desc on the stack of the call: it is gone when the push runs
    unsafe {
        assert_eq!(ti_open_in_key(1, 0, 8, 10), 0);
        ti_release(10);
        ti_defer_poll();
        assert_eq!(
            CStr::from_ptr(ti_pushed_initial()).to_str().unwrap(),
            "KICK"
        );
    }
}

#[test]
fn an_owner_that_unloads_while_its_input_waits_for_the_push_frees_the_input() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open_in_key(1, 0, 8, 10), 0);
        ti_drop(1);
        assert_eq!(ti_defer_pending(), 0);
        ti_release(10);
        ti_defer_poll();
        assert_eq!(ti_pushes(), 0, "no screen for an unloaded module");
        assert_eq!(ti_open(2, 1, 8), 0, "the input is free");
    }
}

#[test]
fn a_deferred_push_that_fails_frees_the_input() {
    let _g = fresh();
    unsafe {
        assert_eq!(ti_open_in_key(1, 0, 8, 10), 0);
        ti_set_push_result(E_NOMEM);
        ti_release(10);
        ti_defer_poll();
        assert_eq!(ti_done_calls(0), 0);
        ti_set_push_result(0);
        assert_eq!(ti_open(1, 0, 8), 0);
    }
}
