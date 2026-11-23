use super::canvas::{Canvas, Clipped, MASK_WORDS, ORIGIN_X, WIDTH};
use super::dots::dots;
use super::frame::Frame;
use super::page::{NativePage, FLAG_STOCK_OVERLAY};
use super::surface::{Stock, Surface};
use super::table::PageTable;
use super::view::{Content, View};

/// What the hook does after `draw`: 0 resume stock, 1 handled, 2 request stock first, then `overlay`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Draw {
    Stock = 0,
    Handled = 1,
    StockThenOverlay = 2,
}

fn paint<S: Surface>(
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    page: &NativePage,
    track: u32,
    content: &mut impl Content<S>,
) {
    let width = Frame::width_of(page);
    let f = Frame::read(view, page, track, width);
    let origin_x = ORIGIN_X - (width - WIDTH);
    let is_overlay = page.flags() & FLAG_STOCK_OVERLAY != 0;
    let mut mask = [0u32; MASK_WORDS];
    let mut clipped = Clipped::new(width);
    {
        let mut canvas = Canvas::new(clip, width, is_overlay, &mut mask);
        content.draw(page, &f, &mut canvas);
    }
    if is_overlay {
        clipped.use_mask(&mask);
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
    if page.flags() & FLAG_STOCK_OVERLAY != 0 {
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
    if page.flags() & FLAG_STOCK_OVERLAY == 0 {
        return;
    }
    let track = view.track();
    if track < 16 {
        paint(view, clip, stock, page, track, content);
    }
}

#[cfg(test)]
mod tests;
