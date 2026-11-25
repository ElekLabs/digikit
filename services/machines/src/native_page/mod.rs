//! Native page logic ported from the image point's `native-page.c` (RC4 decision D8): the page table, the frame the content callback sees, the clipped canvas with
//! its overlay mask, the page dots and the turn dispatch. Everything is pure; firmware access goes through the traits of `view` (the view's fields and calls) and
//! `surface` (the stock draw routines). The service file wires it (`dks/src/native.rs`: the view, the surfaces and the content callbacks over the firmware).
//! Layout: `record` (the page record: field offsets, `check`, `apply`), `claim` (`PageSet`: the owner and index claims of a load), `page` (one `NativePage`), `table` (the `PageTable`: registration and lookup), `frame` (the `dk_page_frame` and its bytes), `canvas` (clipping, planes, mask), `dots`, `paint`, `turn`.
mod bitmap;
mod canvas;
mod claim;
mod dots;
mod event;
mod frame;
mod page;
mod paint;
pub mod record;
mod surface;
mod table;
mod turn;
mod view;

#[cfg(test)]
mod fake_firmware;

pub use bitmap::Bitmap;
pub use canvas::{Canvas, Clipped, HEIGHT, MASK_WORDS, ORIGIN_X, PLANE_WORDS, WIDE_WIDTH, WIDTH};
pub use claim::PageSet;
pub use dots::dots;
pub use event::{PageEvent, EVENT_BYTES};
pub use frame::{Frame, FRAME_BYTES};
pub use page::{NativePage, ENCODERS, FLAG_STOCK_OVERLAY, FLAG_WIDE_CANVAS};
pub use paint::{draw, draw_page, overlay, overlay_page, Draw};
pub use surface::{Stock, Surface};
pub use table::{PageTable, TYPES};
pub use turn::{turn, turn_page, Event};
pub use view::{Content, View};
