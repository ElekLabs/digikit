use super::surface::Surface;
use super::table::PageTable;
use super::view::{Content, View};

/// The encoder event as the firmware delivers it: `control` is the 1-based encoder (word at `+0xc`), `delta` (`+0x10`) and `coarse` (byte at `+0x14`).
pub struct Event {
    pub control: u32,
    pub delta: i32,
    pub coarse: bool,
}

/// 0 lets the stock edit run (one native PARAM edit), 1 consumes the event. A LOCAL binding calls the content's event callback and invalidates the view;
/// an unused encoder on a native page is consumed so it cannot edit a hidden parameter.
pub fn turn<S: Surface>(
    table: &PageTable,
    view: &impl View,
    event: &Event,
    content: &mut impl Content<S>,
) -> u32 {
    let Some(page) = table.selected(view) else {
        return 0;
    };
    let encoder = event.control.wrapping_sub(1);
    if encoder >= 8 {
        return 0;
    }
    for b in page.bindings().iter() {
        if b[0] != 0 && b[2] == encoder {
            if b[0] == 1 {
                return 0;
            }
            content.event(page, view.track(), b[1], event.delta, event.coarse);
            view.invalidate();
            return 1;
        }
    }
    1
}

#[cfg(test)]
mod tests;
