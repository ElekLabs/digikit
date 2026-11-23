//! Native page logic ported from the image point's `native-page.c` (RC4 decision D8): the page table, the frame the content callback sees, the clipped canvas with
//! its overlay mask, the page dots and the turn dispatch. Everything is pure; firmware access goes through the traits of `view` (the view's fields and calls) and
//! `surface` (the stock draw routines). It is not linked into the flashed point yet: the C file stays in use until the service wires it.
//! Layout: `table` (registration and lookup), `frame` (the `dk_page_frame` and its bytes), `canvas` (clipping, planes, mask), `dots`, `paint`, `turn`.
mod canvas;
mod dots;
mod frame;
mod paint;
mod surface;
mod table;
mod turn;
mod view;

#[cfg(test)]
mod tests;

pub use canvas::{Canvas, Clipped, HEIGHT, MASK_WORDS, ORIGIN_X, PLANE_WORDS, WIDE_WIDTH, WIDTH};
pub use dots::dots;
pub use frame::Frame;
pub use paint::{draw, overlay, Draw};
pub use surface::{Stock, Surface};
pub use table::{NativePage, PageTable, FLAG_STOCK_OVERLAY, FLAG_WIDE_CANVAS, TYPES};
pub use turn::{turn, Event};
pub use view::{Content, View};
