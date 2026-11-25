use super::*;

const LAYOUT: Layout = Layout {
    kit_table: 0x1000,
    sound_off: 0x28,
    sound_stride: 0x66,
    kit_values_bytes: 0x50,
};

#[test]
fn track_and_kit_words_use_different_live_pointers() {
    assert_eq!(LAYOUT.locate(1, 2, 4, 4, 16), Ok((0x1000, 0x110)));
    assert_eq!(LAYOUT.locate(2, 0, 4, 4, 16), Ok((0x1004, 8)));
}

#[test]
fn unmapped_layout_and_out_of_range_requests_are_refused() {
    let unmapped = Layout {
        kit_table: 0,
        ..LAYOUT
    };
    assert_eq!(unmapped.locate(1, 0, 0, 4, 16), Err(E_NOT_FOUND));
    assert_eq!(LAYOUT.locate(1, 4, 0, 4, 16), Err(E_RANGE));
    assert_eq!(LAYOUT.locate(1, 0, 16, 4, 16), Err(E_RANGE));
    assert_eq!(LAYOUT.locate(2, 0, 40, 4, 16), Err(E_RANGE));
    assert_eq!(LAYOUT.locate(2, 1, 0, 4, 16), Err(E_RANGE));
}
