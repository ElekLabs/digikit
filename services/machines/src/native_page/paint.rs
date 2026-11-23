use super::canvas::*;
use super::dots::dots;
use super::frame::Frame;
use super::surface::{Stock, Surface};
use super::table::{NativePage, PageTable, FLAG_STOCK_OVERLAY, FLAG_WIDE_CANVAS};
use super::view::{Content, View};

/// What the hook does after `draw`: 0 resume stock, 1 handled, 2 request stock first, then `overlay`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Draw {
    Stock = 0,
    Handled = 1,
    StockThenOverlay = 2,
}

/// The frame the content sees: PARAM values by encoder position, the level and the word snapshot.
fn frame(view: &impl View, page: &NativePage, track: u32, width: u32) -> Frame {
    let mut f = Frame {
        track,
        params: [0; 8],
        width,
        height: HEIGHT as u32,
        word_count: page.word_count,
        word_values: [0; 8],
        level: view.value(page.level_code()),
    };
    for b in page.bindings.iter().filter(|b| b[0] == 1) {
        f.params[b[2] as usize] = view.value(b[1]);
    }
    for i in 0..page.word_count as usize {
        f.word_values[i] = view.value(page.word_codes[i]);
    }
    f
}

/// The frame of a page for host tests and the paint below.
pub(super) fn frame_of(view: &impl View, page: &NativePage, track: u32) -> Frame {
    let width = if page.flags & FLAG_WIDE_CANVAS != 0 {
        WIDE_WIDTH
    } else {
        WIDTH
    };
    frame(view, page, track, width)
}

fn paint<S: Surface>(
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    page: &NativePage,
    track: u32,
    content: &mut impl Content<S>,
) {
    let f = frame_of(view, page, track);
    let width = f.width;
    let origin_x = ORIGIN_X - (width - WIDTH);
    let is_overlay = page.flags & FLAG_STOCK_OVERLAY != 0;
    let mut mask = [0u32; MASK_WORDS];
    let mut clipped = Clipped::new(width);
    {
        let mut canvas = Canvas {
            surface: clip,
            width,
            overlay: is_overlay,
            mask: &mut mask,
        };
        content.draw(page, &f, &mut canvas);
    }
    if is_overlay {
        let n = width as usize * 2;
        clipped.plane1[..n].copy_from_slice(&mask[..n]);
    }
    if !is_overlay {
        stock.fill_rect(origin_x as i32, 0, 127, 52, 0);
    }
    stock.blit(&clipped, origin_x);
    if !is_overlay {
        dots(view.page_index(), |a, b, c, d, e| {
            stock.fill_rect(a, b, c, d, e)
        });
    }
}

/// The draw hook: the content draws on `clip`; the result is composited on the firmware canvas `stock` (clear the area unless an overlay, blit, page dots).
pub fn draw<S: Surface>(
    table: &PageTable,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    content: &mut impl Content<S>,
) -> Draw {
    let Some(page) = table.selected(view) else {
        return Draw::Stock;
    };
    let track = view.track();
    if track >= 16 {
        return Draw::Stock;
    }
    if page.flags & FLAG_STOCK_OVERLAY != 0 {
        return Draw::StockThenOverlay;
    }
    paint(view, clip, stock, page, track, content);
    Draw::Handled
}

/// The overlay half, after the stock draw ran; revalidates the view first.
pub fn overlay<S: Surface>(
    table: &PageTable,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    content: &mut impl Content<S>,
) {
    let Some(page) = table.selected(view) else {
        return;
    };
    if page.flags & FLAG_STOCK_OVERLAY == 0 {
        return;
    }
    let track = view.track();
    if track < 16 {
        paint(view, clip, stock, page, track, content);
    }
}
