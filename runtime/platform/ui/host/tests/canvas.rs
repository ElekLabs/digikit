//! The canvas of core/ui: top-left origin for every call, converted in one place, and clipped to the screen.
use dkui_host::session;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn cv_begin(rect_from_bottom: c_int);
    fn cv_end();
    fn cv_log() -> *const c_char;
    fn cv_width() -> u32;
    fn cv_height() -> u32;
    fn cv_text(x: c_int, y: c_int, s: *const c_char);
    fn cv_fill(x0: c_int, y0: c_int, x1: c_int, y1: c_int, c: c_int);
    fn cv_invert(x0: c_int, y0: c_int, x1: c_int, y1: c_int);
    fn cv_frame(x0: c_int, y0: c_int, x1: c_int, y1: c_int, c: c_int);
    fn cv_text_width(s: *const c_char) -> c_int;
    fn cv_text_in(x: c_int, y: c_int, font: c_int, s: *const c_char);
    fn cv_text_width_in(font: c_int, s: *const c_char) -> c_int;
}

fn log() -> Vec<String> {
    unsafe { CStr::from_ptr(cv_log()) }
        .to_string_lossy()
        .lines()
        .map(String::from)
        .collect()
}

fn text(x: i32, y: i32, s: &str) {
    let s = CString::new(s).unwrap();
    unsafe { cv_text(x, y, s.as_ptr()) }
}

#[test]
fn the_canvas_has_the_size_of_the_screen() {
    let _g = session();
    unsafe { cv_begin(0) };
    assert_eq!(unsafe { (cv_width(), cv_height()) }, (128, 64));
    unsafe { cv_end() };
}

#[test]
fn text_y_is_the_top_row_and_the_firmware_gets_the_bottom_edge_counted_up() {
    let _g = session();
    unsafe { cv_begin(0) };
    text(3, 0, "A"); // top row 0, 6 rows high: bottom edge at 58 up from the bottom
    text(3, 58, "B"); // last 6 rows of the screen: bottom edge at row 0
    assert_eq!(log(), ["T 3 58 A", "T 3 0 B"]);
    unsafe { cv_end() };
}

#[test]
fn text_off_the_screen_is_dropped_and_text_that_touches_it_goes_on() {
    let _g = session();
    unsafe { cv_begin(0) };
    text(0, 64, "A"); // below
    text(0, -6, "A"); // above (its 6 rows end at -1)
    text(128, 10, "A"); // right of the screen
    text(-4, 10, "A"); // 4 wide, ends at x = 0: nothing on the screen
    text(0, 10, "");
    assert!(log().is_empty());
    text(-3, 10, "A"); // one column on the screen
    text(0, -5, "A"); // one row on the screen
    assert_eq!(log().len(), 2);
    unsafe { cv_end() };
}

#[test]
fn fill_is_clipped_and_takes_its_corners_in_any_order() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_fill(10, 20, 5, 2, 1) };
    unsafe { cv_fill(-10, -10, 500, 500, 0) };
    unsafe { cv_fill(200, 0, 300, 10, 1) }; // outside
    unsafe { cv_fill(-5, -5, -1, 3, 1) }; // outside
    assert_eq!(log(), ["F 5 2 10 20 1", "F 0 0 127 63 0"]);
    unsafe { cv_end() };
}

#[test]
fn rectangle_rows_follow_the_firmware_origin_when_it_counts_from_the_bottom() {
    let _g = session();
    unsafe { cv_begin(1) };
    unsafe { cv_fill(0, 0, 9, 0, 1) }; // the top row is the firmware's row 63
    unsafe { cv_invert(0, 60, 9, 63) }; // the last four rows are rows 0..3
    unsafe { cv_fill(0, 2, 9, 5, 1) };
    assert_eq!(log(), ["F 0 63 9 63 1", "I 0 0 9 3", "F 0 58 9 61 1"]);
    unsafe { cv_end() };
}

#[test]
fn invert_is_clipped_to_the_screen() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_invert(120, 60, 140, 70) };
    assert_eq!(log(), ["I 120 60 127 63"]);
    unsafe { cv_end() };
}

#[test]
fn a_frame_is_four_lines_without_overlap() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_frame(10, 20, 30, 40, 1) };
    assert_eq!(
        log(),
        [
            "F 10 20 30 20 1",
            "F 10 40 30 40 1",
            "F 10 21 10 39 1",
            "F 30 21 30 39 1"
        ]
    );
    unsafe { cv_end() };
}

#[test]
fn a_frame_that_leaves_the_screen_draws_only_the_edges_on_it() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_frame(-10, -10, 20, 20, 1) };
    // the top and the left edge are off the screen; the bottom and the right edge are on it and cut at the screen's edge
    assert_eq!(log(), ["F 0 20 20 20 1", "F 20 0 20 19 1"]);
    unsafe { cv_end() };
}

#[test]
fn a_one_pixel_frame_and_a_thin_frame() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_frame(5, 5, 5, 5, 1) };
    unsafe { cv_frame(5, 8, 9, 9, 0) };
    assert_eq!(log(), ["F 5 5 5 5 1", "F 5 8 9 8 0", "F 5 9 9 9 0"]);
    unsafe { cv_end() };
}

#[test]
fn text_width_asks_the_firmware_and_null_is_zero() {
    let _g = session();
    unsafe { cv_begin(0) };
    assert_eq!(unsafe { cv_text_width(c"ABC".as_ptr()) }, 12);
    assert_eq!(unsafe { cv_text_width(std::ptr::null()) }, 0);
    unsafe { cv_end() };
}

#[test]
fn after_the_draw_the_canvas_does_nothing() {
    let _g = session();
    unsafe { cv_begin(0) };
    unsafe { cv_end() };
    text(0, 0, "A");
    unsafe { cv_fill(0, 0, 5, 5, 1) };
    unsafe { cv_frame(0, 0, 5, 5, 1) };
    assert_eq!(unsafe { cv_text_width(c"A".as_ptr()) }, 0);
    assert!(log().is_empty());
}

fn text_in(x: i32, y: i32, font: i32, s: &str) {
    let s = CString::new(s).unwrap();
    unsafe { cv_text_in(x, y, font, s.as_ptr()) }
}

#[test]
fn large_text_y_is_the_top_row_of_its_taller_box() {
    let _g = session();
    unsafe { cv_begin(0) };
    text_in(3, 0, 1, "A"); // top row 0, 15 rows high: the bottom edge is 49 up from the bottom
    text_in(3, 49, 1, "B"); // the last 15 rows: bottom edge at row 0
    text_in(3, 10, 0, "C"); // the small font keeps its 6 rows: 64 - 10 - 6
    assert_eq!(log(), ["L 3 49 A", "L 3 0 B", "T 3 48 C"]);
    unsafe { cv_end() };
}

#[test]
fn large_text_is_clipped_by_its_own_height_and_width() {
    let _g = session();
    unsafe { cv_begin(0) };
    text_in(0, 64, 1, "A"); // below
    text_in(0, -15, 1, "A"); // above: its 15 rows end at -1
    text_in(128, 0, 1, "A"); // right of the screen
    text_in(-8, 0, 1, "A"); // 8 wide, ends at x = 0
    text_in(0, 0, 1, "");
    assert!(log().is_empty());
    text_in(-7, 0, 1, "A"); // one column on the screen
    text_in(0, -14, 1, "A"); // one row on the screen
                             // a small text at the same y is 6 rows high: -14 is off the screen for it
    text_in(0, -14, 0, "A");
    assert_eq!(log().len(), 2);
    unsafe { cv_end() };
}

#[test]
fn text_width_in_asks_the_firmware_with_the_font() {
    let _g = session();
    unsafe { cv_begin(0) };
    assert_eq!(unsafe { cv_text_width_in(0, c"ABC".as_ptr()) }, 12);
    assert_eq!(unsafe { cv_text_width_in(1, c"ABC".as_ptr()) }, 24);
    assert_eq!(
        unsafe { cv_text_width(c"ABC".as_ptr()) },
        12,
        "text_width stays small"
    );
    assert_eq!(unsafe { cv_text_width_in(1, std::ptr::null()) }, 0);
    // a font number the canvas does not know is the small font
    assert_eq!(unsafe { cv_text_width_in(7, c"ABC".as_ptr()) }, 12);
    unsafe { cv_end() };
}
