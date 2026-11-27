//! The event struct the content callback reads.
use super::*;
use core::mem::{offset_of, size_of};

#[test]
fn the_event_is_six_words_in_the_order_of_the_sdk_struct() {
    assert_eq!(size_of::<PageEvent>(), EVENT_BYTES);
    let e = PageEvent::new(5, 9, -2, true);
    assert_eq!(
        (e.version, e.size, e.track, e.control, e.delta, e.coarse),
        (1, 24, 5, 9, -2, 1)
    );
    assert_eq!(offset_of!(PageEvent, control), 12);
    assert_eq!(offset_of!(PageEvent, coarse), 20);
}
