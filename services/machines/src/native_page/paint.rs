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
    clipped: &mut Clipped,
    page: &NativePage,
    track: u32,
    content: &mut impl Content<S>,
) {
    let width = Frame::width_of(page);
    let f = Frame::read(view, page, track, width);
    let origin_x = ORIGIN_X - (width - WIDTH);
    let is_overlay = page.flags() & FLAG_STOCK_OVERLAY != 0;
    let mut mask = [0u32; MASK_WORDS];
    clipped.reset(width);
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
    stock.blit(clipped, origin_x);
    if !is_overlay {
        dots(view.page_index(), |a, b, c, d, e| {
            stock.fill_rect(a, b, c, d, e)
        });
    }
}

/// The draw hook: the content draws on `clip`, whose bitmap is the planes of `clipped`; the result is composited on the firmware canvas `stock` (clear the area unless an
/// overlay, blit, page dots).
pub fn draw<S: Surface>(
    table: &PageTable,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    clipped: &mut Clipped,
    content: &mut impl Content<S>,
) -> Draw {
    match table.selected(view) {
        Some(page) => draw_page(page, view, clip, stock, clipped, content),
        None => Draw::Stock,
    }
}

/// `draw` for a page the caller took out of the table (`selected`): the content may call back into whatever holds the table, so the caller lets go of it first.
pub fn draw_page<S: Surface>(
    page: &NativePage,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    clipped: &mut Clipped,
    content: &mut impl Content<S>,
) -> Draw {
    let track = view.track();
    if track >= 16 {
        return Draw::Stock;
    }
    if page.flags() & FLAG_STOCK_OVERLAY != 0 {
        return Draw::StockThenOverlay;
    }
    paint(view, clip, stock, clipped, page, track, content);
    Draw::Handled
}

/// The overlay half, after the stock draw ran; revalidates the view first.
pub fn overlay<S: Surface>(
    table: &PageTable,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    clipped: &mut Clipped,
    content: &mut impl Content<S>,
) {
    if let Some(page) = table.selected(view) {
        overlay_page(page, view, clip, stock, clipped, content);
    }
}

/// `overlay` for a page the caller took out of the table.
pub fn overlay_page<S: Surface>(
    page: &NativePage,
    view: &impl View,
    clip: &mut S,
    stock: &mut impl Stock,
    clipped: &mut Clipped,
    content: &mut impl Content<S>,
) {
    if page.flags() & FLAG_STOCK_OVERLAY == 0 {
        return;
    }
    let track = view.track();
    if track < 16 {
        paint(view, clip, stock, clipped, page, track, content);
    }
}

#[cfg(test)]
mod tests;
