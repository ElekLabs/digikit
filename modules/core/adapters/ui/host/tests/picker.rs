//! The sample picker (sdk/src/sample_picker.c) on the fake <dk/ui.h> and a fake dk_api: its screens, keys and texts.
//! The pure functions (sdk/src/picker_logic.c) have their own tests at the end.
use dkui_host::driver::*;
use dkui_host::*;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn pf_reset(with_path: c_int);
    fn pf_init(handle: u32) -> c_int;
    fn pf_settings() -> *mut DkUiMenu;
    fn pf_fini();
    fn pf_add(path: *const c_char, dir: c_int, size: u32);
    fn pf_finish();
    fn pf_partial();
    fn pf_stale_notify();
    fn pf_fail(rc: c_int);
    fn pf_list_result(rc: c_int);
    fn pf_list_calls() -> c_int;
    fn pf_cancels() -> c_int;
    fn pf_list_dir() -> *const c_char;
    fn pf_client_calls() -> c_int;
    fn pf_track(t: c_int, status: c_int, path: *const c_char, frames: u32, rate: u32, ch: u32);
    fn pf_status(t: c_int, status: c_int);
    fn pf_select_result(rc: c_int);
    fn pf_selects() -> c_int;
    fn pf_unbinds() -> c_int;
    fn pf_last_track() -> c_int;
    fn pf_last_path() -> *const c_char;
    fn pf_ticking() -> c_int;

    fn pk_trunc(out: *mut c_char, name: *const c_char, max: c_int);
    fn pk_size_text(out: *mut c_char, bytes: u32);
    fn pk_secs_text(out: *mut c_char, frames: u32, rate: u32);
    fn pk_join(out: *mut c_char, cap: c_int, dir: *const c_char, leaf: *const c_char) -> c_int;
    fn pk_parent(dir: *mut c_char) -> c_int;
    fn pk_err_tail(out: *mut c_char, code: c_int);
    fn pk_err_info(out: *mut c_char, code: c_int);
    fn pk_track_row(
        out: *mut c_char,
        track: c_int,
        status: c_int,
        refused: c_int,
        name: *const c_char,
        frames: u32,
        rate: u32,
        channels: u32,
    );
}

const E_NOT_FOUND: i32 = -1;
const E_READ: i32 = -3;
const E_FORMAT: i32 = -5;
const E_TOO_BIG: i32 = -13;
const E_LOST: i32 = -15;
const E_POOL: i32 = -16;
const E_CHANGED: i32 = -17;
const E_SEL_PATH: i32 = -40;
const E_CONTRACT: i32 = -48;
const QUEUED: i32 = 1;
const LOADING: i32 = 2;
const READY: i32 = 3;

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}
fn add(path: &str, dir: bool, size: u32) {
    unsafe { pf_add(c(path).as_ptr(), dir as c_int, size) }
}
fn track(t: i32, status: i32, path: &str, frames: u32, rate: u32, ch: u32) {
    unsafe { pf_track(t, status, c(path).as_ptr(), frames, rate, ch) }
}
fn last_path() -> String {
    unsafe { CStr::from_ptr(pf_last_path()) }
        .to_string_lossy()
        .into_owned()
}
fn list_dir() -> String {
    unsafe { CStr::from_ptr(pf_list_dir()) }
        .to_string_lossy()
        .into_owned()
}
/// A row of a list as render() shows it, without the two-character check column.
fn rows() -> Vec<String> {
    render().iter().map(|r| r[2..].to_string()).collect()
}

/// A session with the picker started and its track list open.
fn start() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe {
        pf_reset(1);
        assert_eq!(pf_init(1), 0);
        let m = pf_settings();
        assert_eq!(open(m), 0);
    }
    assert_eq!(render(), vec!["SAMPLES..."]);
    press_yes(0);
    assert_eq!(depth(), 2);
    g
}

/// YES on track t: the browser opens and lists `entries` (path, is_dir, size) to the end.
fn browse(t: usize, entries: &[(&str, bool, u32)]) {
    press_yes(t);
    for (p, d, s) in entries {
        add(p, *d, *s);
    }
    unsafe { pf_finish() };
}

#[test]
fn no_entry_row_without_a_handle_a_client_or_lists() {
    let _g = session();
    unsafe {
        pf_reset(1);
        assert_eq!(pf_init(0), -1, "no table handle");
        open(pf_settings());
        assert!(render().is_empty());
    }
    let _ = depth();
    unsafe {
        pf_reset(1);
        assert_eq!(pf_init(1), 0);
        set_cap(CAP_LIST, false);
        open(pf_settings());
        assert!(render().is_empty(), "no list capability");
    }
}

#[test]
fn a_runtime_without_the_listing_api_leaves_the_row_out() {
    let _g = session();
    unsafe {
        pf_reset(1);
        // an Api table that ends before files_client (API 1.3)
        extern "C" {
            fn pf_short_api();
        }
        pf_short_api();
        assert_eq!(pf_init(1), -1);
        open(pf_settings());
        assert!(render().is_empty());
    }
}

#[test]
fn the_listing_client_is_acquired_once_in_init() {
    let _g = start();
    assert_eq!(unsafe { pf_client_calls() }, 1);
    press_yes(0);
    assert_eq!(unsafe { pf_client_calls() }, 1);
}

#[test]
fn track_list_shows_sixteen_rows_with_the_state_of_each() {
    let _g = session();
    unsafe {
        pf_reset(1);
        track(0, READY, "KICKS/KICK808", 57600, 48000, 2);
        track(1, READY, "SNARE_A", 19200, 48000, 1);
        track(2, LOADING, "LONG/PATH/LOOP", 0, 0, 0);
        track(3, QUEUED, "X", 0, 0, 0);
        track(4, E_POOL, "BIGONE", 0, 0, 0);
        pf_init(1);
        open(pf_settings());
    }
    press_yes(0);
    let r = rows();
    assert_eq!(r.len(), 16);
    assert_eq!(r[0], "01 KICK808 S 1.2s");
    assert_eq!(r[1], "02 SNARE_A M 0.4s");
    assert_eq!(r[2], "03 LOOP LOADING...");
    assert_eq!(r[3], "04 X QUEUED");
    assert_eq!(r[4], "05 BIGONE !POOL FULL");
    assert_eq!(r[5], "06 -");
    assert_eq!(r[15], "16 -");
    assert!(r.iter().all(|x| x.len() <= 23));
}

#[test]
fn without_sample_track_path_the_rows_show_no_names() {
    let _g = session();
    unsafe {
        pf_reset(0);
        track(0, READY, "KICKS/KICK808", 48000, 48000, 2);
        pf_init(1);
        open(pf_settings());
    }
    press_yes(0);
    assert_eq!(rows()[0], "01 S 1.0s");
}

#[test]
fn row_text_per_state_and_error_code() {
    let cases = [
        (E_NOT_FOUND, "!MISSING", "FILE NOT FOUND"),
        (E_READ, "!READ ERROR", "CARD READ FAILED"),
        (E_FORMAT, "!BAD FORMAT", "NEEDS PCM16 48K MONO OR STEREO"),
        (E_TOO_BIG, "!TOO BIG", "DOES NOT FIT THE POOL"),
        (E_LOST, "!LOST", "BACKING LOST, RELOAD FAILED"),
        (E_POOL, "!POOL FULL", "POOL FULL: CLEAR A TRACK"),
        (E_CHANGED, "!CHANGED", "FILE CHANGED SINCE SELECTION"),
        (E_SEL_PATH, "!BAD PATH", "PATH TOO LONG OR BAD"),
        (
            E_CONTRACT,
            "NO SAMPLE POOL",
            "NO SAMPLE POOL ON THIS DEVICE",
        ),
        (-99, "!ERR -99", "ERR -99"),
    ];
    for (code, tail, info) in cases {
        let mut b = [0 as c_char; 32];
        unsafe { pk_err_tail(b.as_mut_ptr(), code) };
        assert_eq!(
            unsafe { CStr::from_ptr(b.as_ptr()) }.to_str().unwrap(),
            tail
        );
        unsafe { pk_err_info(b.as_mut_ptr(), code) };
        assert_eq!(
            unsafe { CStr::from_ptr(b.as_ptr()) }.to_str().unwrap(),
            info
        );
        assert!(tail.len() <= 14);
        let mut row = [0 as c_char; 24];
        unsafe { pk_track_row(row.as_mut_ptr(), 4, code, 0, c("LOOP").as_ptr(), 0, 0, 0) };
        let s = unsafe { CStr::from_ptr(row.as_ptr()) }.to_str().unwrap();
        assert!(s.ends_with(tail) && s.len() <= 23, "{s}");
    }
}

#[test]
fn a_refusal_wins_over_the_status_and_a_long_name_gives_way_to_it() {
    let mut row = [0 as c_char; 24];
    unsafe {
        pk_track_row(
            row.as_mut_ptr(),
            0,
            READY,
            E_POOL,
            c("KICK_LONG_NAME_FINAL").as_ptr(),
            100,
            48000,
            2,
        )
    };
    let s = unsafe { CStr::from_ptr(row.as_ptr()) }.to_str().unwrap();
    assert_eq!(s, "01 KICK_~NAL !POOL FULL");
    unsafe { pk_track_row(row.as_mut_ptr(), 9, 0, E_CONTRACT, c("").as_ptr(), 0, 0, 0) };
    let s = unsafe { CStr::from_ptr(row.as_ptr()) }.to_str().unwrap();
    assert_eq!(s, "10 NO SAMPLE POOL");
}

#[test]
fn browser_lists_folders_first_sorted_without_case() {
    let _g = start();
    press_yes(0);
    assert_eq!(rows(), vec!["..  /", "READING..."]);
    assert_eq!(list_dir(), "");
    add("zeta", false, 2048);
    add("Bass", true, 0);
    add("apple", false, 100 * 1024);
    add("alpha", true, 0);
    add("Beta", false, 1500 * 1024);
    unsafe { pf_finish() };
    assert_eq!(
        rows(),
        vec![
            "..  /",
            "> alpha",
            "> Bass",
            "  apple 100K",
            "  Beta 1.5M",
            "  zeta 2K"
        ]
    );
}

#[test]
fn rows_stay_hidden_until_the_listing_ends() {
    let _g = start();
    press_yes(0);
    add("a", false, 1);
    unsafe { pf_partial() };
    assert_eq!(rows(), vec!["..  /", "READING..."]);
    unsafe { pf_finish() };
    assert_eq!(rows().len(), 2);
    assert_eq!(rows()[1], "  a 1K");
}

#[test]
fn a_long_name_keeps_its_start_and_end() {
    let _g = start();
    browse(
        0,
        &[
            ("KICK_LONG_NAME_THAT_ENDS_FINAL_MIX", false, 90 * 1024),
            ("FOLDER_WITH_A_VERY_LONG_NAME_HERE", true, 0),
        ],
    );
    let r = rows();
    assert_eq!(r[1], "> FOLDER_WITH~NAME_HERE");
    assert_eq!(r[2], "  KICK_LONG~NAL_MIX 90K");
    assert!(r.iter().all(|x| x.len() <= 23));
}

#[test]
fn the_cap_keeps_the_first_128_in_sort_order_and_counts_the_rest() {
    let _g = start();
    press_yes(0);
    // 200 files arriving in reverse order; a folder arrives last and still sorts first
    for i in (0..200).rev() {
        add(&format!("F{i:03}"), false, 1024);
    }
    add("DIR", true, 0);
    // the fake provider holds 400 rows
    unsafe { pf_finish() };
    let r = rows();
    assert_eq!(r.len(), 1 + 128 + 1);
    assert_eq!(r[1], "> DIR");
    assert_eq!(r[2], "  F000 1K");
    assert_eq!(r[128], "  F126 1K");
    assert_eq!(r[129], "+73 MORE");
}

#[test]
fn a_name_over_47_bytes_is_not_shown_and_counts_as_hidden() {
    let _g = start();
    browse(0, &[(&"N".repeat(48), false, 1024), ("ok", false, 1024)]);
    assert_eq!(rows(), vec!["..  /", "  ok 1K", "+1 MORE"]);
}

#[test]
fn an_empty_folder_and_a_failed_listing() {
    let _g = start();
    browse(0, &[]);
    assert_eq!(rows(), vec!["..  /", "NO SAMPLES HERE"]);
    press_no();
    assert_eq!(depth(), 2);
    press_yes(0);
    add("a", false, 1);
    unsafe { pf_fail(-3) };
    assert_eq!(rows(), vec!["..  /", "LIST ERROR -3"]);
    press_no();
    unsafe { pf_list_result(-13) };
    press_yes(0);
    assert_eq!(rows(), vec!["..  /", "LIST ERROR -13"]);
}

#[test]
fn folders_open_and_no_goes_up_then_out_at_the_root() {
    let _g = start();
    browse(2, &[("DRUMS", true, 0)]);
    let calls = unsafe { pf_list_calls() };
    press_yes(1);
    assert_eq!(list_dir(), "DRUMS");
    assert_eq!(unsafe { pf_list_calls() }, calls + 1);
    assert_eq!(rows(), vec!["..  /DRUMS", "READING..."]);
    add("KICKS", true, 0);
    unsafe { pf_finish() };
    press_yes(1);
    assert_eq!(list_dir(), "DRUMS/KICKS");
    add("K1", false, 2048);
    unsafe { pf_finish() };
    assert_eq!(rows(), vec!["..  /DRUMS/KICKS", "  K1 2K"]);
    // NO goes up one folder at a time and stays in the browser
    press_no();
    assert_eq!((list_dir().as_str(), depth()), ("DRUMS", 3));
    // the ".." row does the same
    press_yes(0);
    assert_eq!((list_dir().as_str(), depth()), ("", 3));
    // at the root NO and ".." leave the browser
    press_no();
    assert_eq!(depth(), 2);
    assert_eq!(title(), "SAMPLES");
    press_yes(0);
    press_yes(0);
    assert_eq!(depth(), 2);
}

#[test]
fn a_new_folder_cancels_the_listing_that_still_runs() {
    let _g = start();
    press_yes(0);
    let c0 = unsafe { pf_cancels() };
    press_no(); // leaves while reading
    assert_eq!(unsafe { pf_cancels() }, c0 + 1);
    press_yes(0);
    add("D", true, 0);
    unsafe { pf_finish() };
    press_yes(1);
    assert_eq!(
        unsafe { pf_cancels() },
        c0 + 1,
        "a finished listing is not cancelled"
    );
    // a notification of the old ticket changes nothing
    unsafe { pf_stale_notify() };
    assert_eq!(rows()[1], "READING...");
}

#[test]
fn yes_on_a_file_selects_the_full_path_and_returns_to_the_track_list() {
    let _g = start();
    browse(3, &[("DRUMS", true, 0)]);
    press_yes(1);
    add("KICK_01", false, 90 * 1024);
    unsafe { pf_finish() };
    press_yes(1);
    assert_eq!(unsafe { pf_selects() }, 1);
    assert_eq!(
        (unsafe { pf_last_track() }, last_path().as_str()),
        (3, "DRUMS/KICK_01")
    );
    assert_eq!(depth(), 2);
    assert_eq!(rows()[3], "04 KICK_01 QUEUED");
}

#[test]
fn the_browser_never_changes_the_track_by_browsing() {
    let _g = start();
    browse(1, &[("A", false, 1024)]);
    press_no();
    assert_eq!(unsafe { pf_selects() }, 0);
    assert_eq!(unsafe { pf_unbinds() }, 0);
}

#[test]
fn a_folder_has_no_side_panel_and_a_file_has_select_and_info() {
    let _g = start();
    browse(0, &[("D", true, 0), ("S1", false, 4096)]);
    let before = depth();
    press_right(1); // the folder
    assert_eq!(depth(), before);
    press_right(2);
    assert_eq!(render(), vec!["SELECT", "INFO"]);
    choose_panel_row(0);
    assert_eq!((unsafe { pf_selects() }, last_path().as_str()), (1, "S1"));
    assert_eq!(depth(), 2);

    browse(0, &[("D", true, 0), ("S1", false, 4096)]);
    press_right(2);
    choose_panel_row(1);
    assert_eq!(title(), "INFO");
    assert_eq!(render(), vec!["S1", "S1", "SIZE 4096 BYTES"]);
}

#[test]
fn a_select_at_the_path_limit_is_refused_by_the_picker_without_a_call() {
    let _g = start();
    // five folders of 47 bytes make a 239-byte path; a 20-byte file over 255 is too long
    let dir = "d".repeat(47);
    press_yes(0);
    for _ in 0..5 {
        add(&dir, true, 0);
        unsafe { pf_finish() };
        press_yes(1);
    }
    assert_eq!(list_dir().len(), 5 * 47 + 4);
    add(&"f".repeat(20), false, 1024);
    unsafe { pf_finish() };
    press_yes(1);
    assert_eq!(unsafe { pf_selects() }, 0);
    assert_eq!(depth(), 2);
    assert_eq!(rows()[0], "01 !BAD PATH");
    // a file that just fits is passed on
    press_yes(0);
    add(&"g".repeat(15), false, 1024);
    unsafe { pf_finish() };
    press_yes(1);
    assert_eq!(unsafe { pf_selects() }, 1);
    assert_eq!(last_path().len(), 255);
}

#[test]
fn a_refusal_shows_in_the_track_row_and_keeps_the_old_sample() {
    let _g = session();
    unsafe {
        pf_reset(1);
        track(0, READY, "OLD", 48000, 48000, 2);
        pf_init(1);
        open(pf_settings());
        pf_select_result(E_POOL);
    }
    press_yes(0);
    browse(0, &[("NEW", false, 1024)]);
    press_yes(1);
    assert_eq!(unsafe { pf_selects() }, 1);
    assert_eq!(depth(), 2, "no dialog, the browser closed");
    assert!(dialog().is_none());
    assert_eq!(rows()[0], "01 OLD !POOL FULL");
    // the next select that works clears the refusal
    unsafe { pf_select_result(0) };
    browse(0, &[("NEW", false, 1024)]);
    press_yes(1);
    assert_eq!(rows()[0], "01 NEW QUEUED");
}

#[test]
fn a_device_without_a_pool_says_so_in_the_row() {
    let _g = start();
    unsafe { pf_select_result(E_CONTRACT) };
    browse(5, &[("A", false, 1024)]);
    press_yes(1);
    assert_eq!(rows()[5], "06 NO SAMPLE POOL");
    // INFO says it in full
    press_right(5);
    choose_panel_row(1);
    assert_eq!(
        render(),
        vec!["TRACK 6", "STATE", "NO SAMPLE POOL ON THIS", "DEVICE"]
    );
}

#[test]
fn the_timer_runs_while_a_load_runs_and_stops_when_it_ends() {
    let _g = start();
    assert!(unsafe { pf_ticking() } == 0 && tick_periods().is_empty());
    browse(0, &[("A", false, 1024)]);
    press_yes(1);
    assert_eq!(tick_periods(), vec![250]);
    assert_eq!(rows()[0], "01 A QUEUED");
    // the runtime moves on without a key press; the tick shows it
    unsafe { pf_status(0, LOADING) };
    advance_ms(249);
    assert_eq!(rows()[0], "01 A QUEUED");
    advance_ms(1);
    assert_eq!(rows()[0], "01 A LOADING...");
    unsafe { pf_track(0, READY, c("A").as_ptr(), 24000, 48000, 1) };
    advance_ms(250);
    assert_eq!(rows()[0], "01 A M 0.5s");
    assert!(
        tick_periods().is_empty(),
        "the timer stops with the last load"
    );
    // one timer for several loads
    unsafe {
        pf_status(1, LOADING);
        pf_status(2, LOADING);
    }
    press_no(); // out of the track list
    press_yes(0);
    assert_eq!(tick_periods(), vec![250]);
    unsafe { pf_status(1, READY) };
    advance_ms(250);
    assert_eq!(tick_periods(), vec![250]);
    unsafe { pf_status(2, READY) };
    advance_ms(250);
    assert!(tick_periods().is_empty());
}

#[test]
fn leaving_the_track_list_stops_the_timer() {
    let _g = start();
    unsafe { pf_status(0, LOADING) };
    browse(1, &[]);
    press_no();
    press_no(); // browser, then track list
    assert_eq!(depth(), 1);
    advance_ms(250);
    assert!(tick_periods().is_empty());
    unsafe { pf_fini() };
}

#[test]
fn without_a_timer_capability_the_rows_still_show_the_state_on_the_next_key() {
    let _g = session();
    set_cap(CAP_TICK, false);
    unsafe {
        pf_reset(1);
        track(0, LOADING, "A", 0, 0, 0);
        pf_init(1);
        open(pf_settings());
    }
    press_yes(0);
    assert_eq!(rows()[0], "01 A LOADING...");
    assert!(tick_periods().is_empty());
}

#[test]
fn clear_unbinds_with_one_press() {
    let _g = session();
    unsafe {
        pf_reset(1);
        track(2, READY, "A", 100, 48000, 1);
        pf_init(1);
        open(pf_settings());
    }
    press_yes(0);
    press_right(2);
    assert_eq!(render(), vec!["CLEAR", "INFO"]);
    choose_panel_row(0);
    assert_eq!(unsafe { pf_unbinds() }, 1);
    assert_eq!(rows()[2], "03 -");
}

#[test]
fn track_info_page_shows_name_path_format_and_state() {
    let _g = session();
    unsafe {
        pf_reset(1);
        track(2, READY, "DRUMS/KICKS/KICK808", 57600, 48000, 2);
        track(3, E_FORMAT, "X/BAD", 0, 0, 0);
        track(4, LOADING, "L", 0, 0, 0);
        pf_init(1);
        open(pf_settings());
    }
    press_yes(0);
    press_right(2);
    choose_panel_row(1);
    assert_eq!(title(), "INFO");
    assert_eq!(
        render(),
        vec![
            "TRACK 3",
            "KICK808",
            "DRUMS/KICKS/KICK808",
            "STEREO 48000 HZ",
            "57600 FRAMES 1.2s",
            "STATE READY"
        ]
    );
    press_no();
    press_right(3);
    choose_panel_row(1);
    assert_eq!(
        render(),
        vec![
            "TRACK 4",
            "BAD",
            "X/BAD",
            "STATE",
            "NEEDS PCM16 48K MONO OR",
            "STEREO"
        ]
    );
    press_no();
    press_right(4);
    choose_panel_row(1);
    assert_eq!(render().last().unwrap(), "STATE LOADING");
    press_no();
    press_right(9);
    choose_panel_row(1);
    assert_eq!(render(), vec!["TRACK 10", "NO SAMPLE"]);
}

// ---- the pure functions ----

fn text(f: impl FnOnce(*mut c_char)) -> String {
    let mut b = [0 as c_char; 64];
    f(b.as_mut_ptr());
    unsafe { CStr::from_ptr(b.as_ptr()) }
        .to_str()
        .unwrap()
        .to_string()
}

#[test]
fn truncation_keeps_start_and_end() {
    let t = |n: &str, max| text(|o| unsafe { pk_trunc(o, c(n).as_ptr(), max) });
    assert_eq!(t("KICK", 10), "KICK");
    assert_eq!(t("ABCDEFGHIJ", 10), "ABCDEFGHIJ");
    assert_eq!(t("ABCDEFGHIJK", 10), "ABCDE~HIJK");
    assert_eq!(t("ABCDEFGHIJK", 10).len(), 10);
    assert_eq!(t("ABCDEFGHIJK", 2), "AB");
    assert_eq!(t("ABCDEFGHIJK", 0), "");
}

#[test]
fn size_and_length_text() {
    let s = |b| text(|o| unsafe { pk_size_text(o, b) });
    assert_eq!(
        (
            s(0).as_str(),
            s(1).as_str(),
            s(1024).as_str(),
            s(1025).as_str()
        ),
        ("0K", "1K", "1K", "2K")
    );
    assert_eq!(s(999 * 1024), "999K");
    assert_eq!(s(1024 * 1024), "1.0M");
    assert_eq!(s(5 * 1024 * 1024 / 2), "2.5M");
    assert_eq!(s(u32::MAX), "4096.0M");
    let l = |f, r| text(|o| unsafe { pk_secs_text(o, f, r) });
    assert_eq!(l(57600, 48000), "1.2s");
    assert_eq!(l(0, 48000), "0.0s");
    assert_eq!(l(48000 * 99, 48000), "99.0s");
    assert_eq!(l(48000 * 123, 48000), "123s");
    assert_eq!(l(5, 0), "0.0s");
}

#[test]
fn path_join_and_parent() {
    let j = |d: &str, l: &str| {
        let mut b = [0 as c_char; 300];
        let n = unsafe { pk_join(b.as_mut_ptr(), 300, c(d).as_ptr(), c(l).as_ptr()) };
        (
            n,
            unsafe { CStr::from_ptr(b.as_ptr()) }
                .to_str()
                .unwrap()
                .to_string(),
        )
    };
    assert_eq!(j("", "A"), (1, "A".into()));
    assert_eq!(j("A/B", "C"), (5, "A/B/C".into()));
    // 255 bytes fit, 256 do not
    let d = "d".repeat(200);
    assert_eq!(j(&d, &"f".repeat(54)).0, 255);
    assert_eq!(j(&d, &"f".repeat(55)).0, -1);
    assert_eq!(j("A", "").0, -1);
    let mut b = [0 as c_char; 8];
    assert_eq!(
        unsafe { pk_join(b.as_mut_ptr(), 4, c("AB").as_ptr(), c("C").as_ptr()) },
        -1,
        "cap"
    );

    let p = |s: &str| {
        let mut b = [0 as c_char; 64];
        for (i, ch) in s.bytes().enumerate() {
            b[i] = ch as c_char;
        }
        let r = unsafe { pk_parent(b.as_mut_ptr()) };
        (
            r,
            unsafe { CStr::from_ptr(b.as_ptr()) }
                .to_str()
                .unwrap()
                .to_string(),
        )
    };
    assert_eq!(p("A/B/C"), (1, "A/B".into()));
    assert_eq!(p("A"), (1, "".into()));
    assert_eq!(p(""), (0, "".into()));
}
