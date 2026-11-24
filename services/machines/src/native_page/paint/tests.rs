//! Draw and overlay: what the content is given and how the result is composited.
use crate::native_page::fake_firmware::*;
use crate::native_page::*;

#[test]
fn draw_hands_content_the_frame_and_refuses_bad_tracks() {
    let t = table(0, 1);
    let mut v = FakeView::new().set(40, 112 << 8);
    let (mut clip, mut fw, mut m, mut cl) = (
        Clip::default(),
        Firmware::default(),
        Module::default(),
        Clipped::empty(),
    );
    assert_eq!(
        draw(&t, &v, &mut clip, &mut fw, &mut cl, &mut m),
        Draw::Handled
    );
    assert_eq!(m.frames.len(), 1);
    v.track = 16;
    assert_eq!(
        draw(&t, &v, &mut clip, &mut fw, &mut cl, &mut m),
        Draw::Stock
    );
    assert_eq!(m.frames.len(), 1);
    v.track = 0;
    v.ty = 4; // another machine type: stock
    assert_eq!(
        draw(&t, &v, &mut clip, &mut fw, &mut cl, &mut m),
        Draw::Stock
    );
}

#[test]
fn draw_composites_the_clipped_bitmap_and_page_dots() {
    let t = table(0, 1);
    let v = FakeView::new();
    let (mut clip, mut fw, mut m, mut cl) = (
        Clip::default(),
        Firmware::default(),
        Module::default(),
        Clipped::empty(),
    );
    draw(&t, &v, &mut clip, &mut fw, &mut cl, &mut m);
    assert_eq!(fw.fills[0], [25, 0, 127, 52, 0]);
    let (x, w, plane1) = &fw.blits[0];
    assert_eq!((*x, *w), (25, 99));
    assert_eq!(plane1[0], u32::MAX);
    assert_eq!(plane1[1], u32::MAX << 11);
    assert_eq!(
        &fw.fills[1..],
        &[[125, 32, 126, 33, -1], [124, 23, 127, 26, -1]]
    );
}

#[test]
fn wide_canvas_moves_the_origin_left() {
    let t = table(FLAG_WIDE_CANVAS, 1);
    let (mut clip, mut fw, mut m, mut cl) = (
        Clip::default(),
        Firmware::default(),
        Module::default(),
        Clipped::empty(),
    );
    draw(&t, &FakeView::new(), &mut clip, &mut fw, &mut cl, &mut m);
    assert_eq!((fw.blits[0].0, fw.blits[0].1), (17, 107));
    assert_eq!(m.frames[0].width(), 107);
}

#[test]
fn overlay_page_draws_in_two_steps_and_masks_only_filled_rectangles() {
    let t = table(FLAG_STOCK_OVERLAY, 0);
    let mut v = FakeView::new();
    v.index = 0;
    let (mut clip, mut fw, mut m, mut cl) = (
        Clip::default(),
        Firmware::default(),
        Module::default(),
        Clipped::empty(),
    );
    m.rects = std::vec![[0, 0, 22, 19, 1]];
    assert_eq!(
        draw(&t, &v, &mut clip, &mut fw, &mut cl, &mut m),
        Draw::StockThenOverlay
    );
    assert!(m.frames.is_empty() && fw.blits.is_empty());
    overlay(&t, &v, &mut clip, &mut fw, &mut cl, &mut m);
    assert_eq!(m.frames.len(), 1);
    assert!(
        fw.fills.is_empty(),
        "an overlay clears nothing and draws no dots"
    );
    let plane1 = &fw.blits[0].2;
    // x = 0, rows y 0..=19 are the bottom-origin rows 33..=52: band 1, bits 30..=11
    assert_eq!(plane1[0], 0);
    assert_eq!(plane1[1], 0x7fff_f800);
    assert_eq!(plane1[2 * 23], 0);
    // revalidation: the view changed under the stock draw
    v.ty = 9;
    let before = m.frames.len();
    overlay(&t, &v, &mut clip, &mut fw, &mut cl, &mut m);
    assert_eq!(m.frames.len(), before);
}
