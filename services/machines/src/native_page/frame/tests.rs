//! The frame bytes and the layout header the C adapter asserts against `dk_page_frame`.
use super::*;
use crate::native_page::fake_firmware::*;

#[test]
fn frame_reports_param_and_level_independently() {
    let t = table(0, 1);
    let page = t.page(3, 1).unwrap();
    let v = FakeView::new()
        .set(40, 112 << 8)
        .set(10, 12345)
        .set(41, 5)
        .set(42, -6);
    let f = Frame::read(&v, page, 0, 99);
    assert_eq!(
        (f.params()[7], f.level(), f.word_count()),
        (112 << 8, 12345, 2)
    );
    assert_eq!(*f.word_values(), [5, -6, 0, 0, 0, 0, 0, 0]);
    let v = FakeView::new().set(40, 122 << 8).set(10, 12345);
    let g = Frame::read(&v, page, 0, 99);
    assert_eq!((g.params()[7], g.level()), (122 << 8, 12345));
    let v = FakeView::new().set(40, 122 << 8).set(10, 6789);
    assert_eq!(Frame::read(&v, page, 0, 99).level, 6789);
}

#[test]
fn frame_bytes_follow_the_offsets() {
    let t = table(0, 1);
    let v = FakeView::new().set(40, 112 << 8).set(10, 6789).set(41, 5);
    let page = t.page(3, 1).unwrap();
    let bytes = Frame::read(&v, page, 5, 99).to_bytes(0xAA, 0xBB);
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(bytes.len(), FRAME_BYTES);
    assert_eq!(
        (word(OFF_VERSION), word(OFF_SIZE), word(OFF_TRACK)),
        (1, 100, 5)
    );
    assert_eq!(word(OFF_PARAMS + 4 * 7), 112 << 8);
    assert_eq!((word(OFF_CANVAS_WIDTH), word(OFF_CANVAS_HEIGHT)), (99, 53));
    assert_eq!((word(OFF_CANVAS_TEXT), word(OFF_CANVAS_FILL)), (0xAA, 0xBB));
    assert_eq!((word(OFF_WORD_COUNT), word(OFF_WORD_VALUES)), (2, 5));
    assert_eq!(word(OFF_LEVEL), 6789);
    assert_eq!(OFF_LEVEL + 4, FRAME_BYTES, "the level is the last word");
}

/// The header is what the C adapter asserts against the struct, so it must equal the Rust constants.
#[test]
fn layout_header_matches_the_constants() {
    let header =
        include_str!("../../../../../tests/fixtures/modules/native-page/src/frame-layout.h");
    let defined = |name: &str| -> usize {
        header
            .lines()
            .find_map(|l| {
                l.strip_prefix("#define ")?
                    .strip_prefix(name)?
                    .trim()
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("{name} missing from the layout header"))
    };
    for (name, want) in [
        ("NP_FRAME_BYTES", FRAME_BYTES),
        ("NP_OFF_VERSION", OFF_VERSION),
        ("NP_OFF_SIZE", OFF_SIZE),
        ("NP_OFF_TRACK", OFF_TRACK),
        ("NP_OFF_PARAMS", OFF_PARAMS),
        ("NP_OFF_CANVAS_WIDTH", OFF_CANVAS_WIDTH),
        ("NP_OFF_CANVAS_HEIGHT", OFF_CANVAS_HEIGHT),
        ("NP_OFF_CANVAS_TEXT", OFF_CANVAS_TEXT),
        ("NP_OFF_CANVAS_FILL", OFF_CANVAS_FILL),
        ("NP_OFF_WORD_COUNT", OFF_WORD_COUNT),
        ("NP_OFF_WORD_VALUES", OFF_WORD_VALUES),
        ("NP_OFF_LEVEL", OFF_LEVEL),
    ] {
        assert_eq!(defined(name), want, "{name}");
    }
}
