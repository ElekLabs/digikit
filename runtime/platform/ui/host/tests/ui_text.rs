//! The pixel width and the pixel padding of list text (`<dk/platform/ui_text.h>`), which the MODULES list and the choice popups share.
use std::ffi::{c_char, c_int, CString};

#[link(name = "dkuisample", kind = "static")]
extern "C" {
    fn t_px(s: *const c_char) -> c_int;
    fn t_pad(buf: *mut c_char, size: c_int, px: c_int) -> c_int;
    fn t_pad_spaces(buf: *mut c_char, size: c_int, px: c_int) -> c_int;
}

fn px(s: &str) -> i32 {
    let s = CString::new(s).unwrap();
    unsafe { t_px(s.as_ptr()) }
}

/// `text` padded to `width` pixels in a buffer of `size` bytes.
fn padded(text: &str, size: usize, width: i32) -> String {
    let mut buf = vec![0u8; size];
    buf[..text.len()].copy_from_slice(text.as_bytes());
    let n = unsafe { t_pad(buf.as_mut_ptr().cast(), size as c_int, width) } as usize;
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

#[test]
fn widths_follow_the_list_font() {
    assert_eq!(px("AB-_9"), 20);
    assert_eq!(px("A B"), 11);
    assert_eq!(px("A.B~"), 15);
}

#[test]
fn padding_reaches_the_width_with_spaces_and_pad_characters() {
    // a multiple of four: pad characters only
    assert_eq!(padded("AB", 40, 20), "AB\u{1}\u{1}\u{1}");
    // 11 pixels left: a space (3) and two pad characters (8)
    let text = padded("A", 40, 15);
    assert_eq!(px(&text), 15);
    assert!(text.starts_with("A "), "{text:?}");
    // a text that is wide enough gets nothing
    assert_eq!(padded("ABCD", 40, 12), "ABCD");
}

#[test]
fn padding_stops_at_the_buffer() {
    assert_eq!(padded("AB", 5, 100).len(), 4);
}

#[test]
fn space_padding_comes_within_two_pixels_and_never_shows_a_pad_character() {
    let mut buf = vec![0u8; 40];
    buf[..2].copy_from_slice(b"AB");
    let n = unsafe { t_pad_spaces(buf.as_mut_ptr().cast(), 40, 20) } as usize;
    let text = String::from_utf8(buf[..n].to_vec()).unwrap();
    assert_eq!(text, "AB    ");
    assert!(!text.contains('\u{1}'));
    assert!((px(&text) - 20).abs() <= 2);
}
