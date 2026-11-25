use super::page::{NativePage, PageSpec, ENCODERS, FLAG_STOCK_OVERLAY, FLAG_WIDE_CANVAS};
use super::view::View;

/// The machine types a module can have pages for: the run-time types the image reserves (`[limits] machine_rows` of the device data), numbered from `first`.
pub const TYPES: usize = dkrt_sdk::limits::MACHINE_ROWS;

/// The pages a machine type can have (the replacement pair).
pub const PAGES: u32 = 2;

/// The page index and machine type of the page a view shows, read through the two getters: `None` when it shows a page past the native pair (the type is then not read). The one
/// place of this rule: the table's `selected` and the service file's hooks, which read before the owner is entered, take it.
pub fn target(
    page_index: impl FnOnce() -> u32,
    native_type: impl FnOnce() -> i32,
) -> Option<(u32, i32)> {
    let index = page_index();
    if index >= PAGES {
        return None;
    }
    Some((index, native_type()))
}

/// The registered pages by machine type and page index (0 or 1), the mode of each type (1 append-only v1, 2 replacement pair v2) and the count of live pages. A type is the
/// position in the reserved range: `first` is the number of the first reserved type, a type below it or past the range has no pages.
pub struct PageTable {
    first: u32,
    pages: [[NativePage; 2]; TYPES],
    modes: [u8; TYPES],
    armed: u32,
}
impl Default for PageTable {
    fn default() -> Self {
        Self::new(0)
    }
}
impl PageTable {
    /// An empty table for the reserved types from number `first`.
    pub const fn new(first: u32) -> Self {
        const E: NativePage = NativePage::empty();
        const ROW: [NativePage; 2] = [E, E];
        PageTable {
            first,
            pages: [ROW; TYPES],
            modes: [0; TYPES],
            armed: 0,
        }
    }
    pub fn armed(&self) -> bool {
        self.armed != 0
    }
    /// The number of the first reserved type: set once at `start`, before any page is registered.
    pub fn set_first(&mut self, first: u32) {
        self.first = first;
    }
    /// The slot of machine type `ty`, if it is one of the reserved types.
    fn slot(&self, ty: u32) -> Option<usize> {
        let at = ty.checked_sub(self.first)? as usize;
        (at < TYPES).then_some(at)
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
        let Some(at) = self.slot(ty) else {
            return Err(());
        };
        if flags & !(FLAG_STOCK_OVERLAY | FLAG_WIDE_CANVAS) != 0
            || (flags != 0 && mode != 2)
            || (flags & FLAG_STOCK_OVERLAY != 0 && index != 0)
            || (flags & FLAG_WIDE_CANVAS != 0 && index != 1)
            || self.slot(ty).is_none()
            || index >= 2
            || draw == 0
            || event == 0
            || self.pages[at][index as usize].draw() != 0
            || (self.modes[at] != 0 && self.modes[at] != mode)
            || words.len() > 8
            // C indexes `params` and the descriptor with the encoder position unchecked, so a PARAM past H writes outside both; the table refuses it before it writes anything
            || bindings.iter().any(|b| b[0] == 1 && b[2] as usize >= ENCODERS)
        {
            return Err(());
        }
        let slot = &mut self.pages[at][index as usize];
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
        self.modes[at] = mode;
        self.armed += 1;
        Ok(())
    }
    pub fn remove(&mut self, ty: u32) {
        let Some(at) = self.slot(ty) else {
            return;
        };
        for page in &mut self.pages[at] {
            if page.draw() != 0 {
                *page = NativePage::empty();
                self.armed -= 1;
            }
        }
        self.modes[at] = 0;
    }
    pub fn page_count(&self, ty: u32, ordinary: u32) -> u32 {
        match self.slot(ty).map(|at| self.modes[at]) {
            Some(m) if m != 0 => {
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
        let p = self.pages[self.slot(ty)?].get(index as usize)?;
        (p.draw() != 0).then_some(p)
    }
    /// The page the view shows, if it is a native one.
    pub fn selected(&self, view: &impl View) -> Option<&NativePage> {
        // nothing registered: this method does not read the view. The service file's hooks read the machine type on every draw and turn of the machine page, registered or not (accepted)
        if !self.armed() {
            return None;
        }
        let (index, ty) = target(|| view.page_index(), || view.native_type())?;
        self.lookup(index, ty)
    }
    /// The page of a view that showed page `index` of machine type `ty` (a negative type is none): the lookup of [`PageTable::selected`] over values the caller read from the
    /// firmware before it entered the owner.
    pub fn lookup(&self, index: u32, ty: i32) -> Option<&NativePage> {
        if index >= PAGES || ty < 0 {
            return None;
        }
        self.page(ty as u32, index)
    }
    /// The parameter code of encoder `encoder`: -1 resumes the stock mapper, 0 is a consumed mapping for LOCAL or unused.
    pub fn code(&self, view: &impl View, encoder: u32) -> i32 {
        let Some(page) = self.selected(view) else {
            return -1;
        };
        page.encoder_code(encoder)
    }
}

#[cfg(test)]
mod tests;
