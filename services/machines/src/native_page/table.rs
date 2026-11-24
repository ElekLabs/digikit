use super::page::{NativePage, PageSpec, ENCODERS, FLAG_STOCK_OVERLAY, FLAG_WIDE_CANVAS};
use super::view::View;

pub const TYPES: usize = 32;

/// The registered pages by machine type and page index (0 or 1), the mode of each type (1 append-only v1, 2 replacement pair v2) and the count of live pages.
pub struct PageTable {
    pages: [[NativePage; 2]; TYPES],
    modes: [u8; TYPES],
    armed: u32,
}
impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}
impl PageTable {
    pub const fn new() -> Self {
        const E: NativePage = NativePage::empty();
        const ROW: [NativePage; 2] = [E, E];
        PageTable {
            pages: [ROW; TYPES],
            modes: [0; TYPES],
            armed: 0,
        }
    }
    pub fn armed(&self) -> bool {
        self.armed != 0
    }
    /// Registers a page. `Err(())` is the C adapter's -1.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &mut self,
        ty: u32,
        index: u32,
        mode: u8,
        flags: u32,
        title: &[u8],
        draw: u32,
        event: u32,
        bindings: &[[u32; 3]; ENCODERS],
        words: &[u32],
        arg: u32,
        with_arg: bool,
    ) -> Result<(), ()> {
        if flags & !(FLAG_STOCK_OVERLAY | FLAG_WIDE_CANVAS) != 0
            || (flags != 0 && mode != 2)
            || (flags & FLAG_STOCK_OVERLAY != 0 && index != 0)
            || (flags & FLAG_WIDE_CANVAS != 0 && index != 1)
            || ty as usize >= TYPES
            || index >= 2
            || draw == 0
            || event == 0
            || self.pages[ty as usize][index as usize].draw() != 0
            || (self.modes[ty as usize] != 0 && self.modes[ty as usize] != mode)
            || words.len() > 8
            // C indexes `params` and the descriptor with the encoder position unchecked, so a PARAM past H writes outside both; the table refuses it before it writes anything
            || bindings.iter().any(|b| b[0] == 1 && b[2] as usize >= ENCODERS)
        {
            return Err(());
        }
        let slot = &mut self.pages[ty as usize][index as usize];
        *slot = NativePage::build(&PageSpec {
            title,
            flags,
            draw,
            event,
            bindings,
            words,
            arg,
            with_arg,
        });
        slot.anchor();
        self.modes[ty as usize] = mode;
        self.armed += 1;
        Ok(())
    }
    pub fn remove(&mut self, ty: u32) {
        if ty as usize >= TYPES {
            return;
        }
        for page in &mut self.pages[ty as usize] {
            if page.draw() != 0 {
                *page = NativePage::empty();
                self.armed -= 1;
            }
        }
        self.modes[ty as usize] = 0;
    }
    pub fn page_count(&self, ty: u32, ordinary: u32) -> u32 {
        match self.modes.get(ty as usize) {
            Some(&m) if m != 0 => {
                if m == 2 {
                    2
                } else {
                    ordinary + 1
                }
            }
            _ => ordinary,
        }
    }
    pub fn page(&self, ty: u32, index: u32) -> Option<&NativePage> {
        let p = self.pages.get(ty as usize)?.get(index as usize)?;
        (p.draw() != 0).then_some(p)
    }
    /// The page the view shows, if it is a native one.
    pub fn selected(&self, view: &impl View) -> Option<&NativePage> {
        // nothing registered: the view is not even read (a hook runs on every draw and turn of the machine page)
        if !self.armed() {
            return None;
        }
        let index = view.page_index();
        if index >= 2 {
            return None;
        }
        let ty = view.native_type();
        if ty < 0 {
            return None;
        }
        self.page(ty as u32, index)
    }
    /// The parameter code of encoder `encoder`: -1 resumes the stock mapper, 0 is a consumed mapping for LOCAL or unused.
    pub fn code(&self, view: &impl View, encoder: u32) -> i32 {
        let Some(page) = self.selected(view) else {
            return -1;
        };
        if encoder >= 8 {
            return -1;
        }
        page.bindings()
            .iter()
            .find(|b| b[0] == 1 && b[2] == encoder)
            .map_or(0, |b| b[1] as i32)
    }
}

#[cfg(test)]
mod tests;
