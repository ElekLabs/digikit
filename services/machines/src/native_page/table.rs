pub const TYPES: usize = 32;
pub const FLAG_STOCK_OVERLAY: u32 = 1;
pub const FLAG_WIDE_CANVAS: u32 = 2;
const LEVEL_CODE: u32 = 10;

/// One registered page. The binding rows are (kind, code or local id, encoder position): kind 1 is a PARAM, kind 2 a LOCAL control, 0 unused.
#[derive(Clone)]
pub struct NativePage {
    pub(super) draw: u32,
    pub(super) event: u32,
    pub(super) flags: u32,
    pub(super) arg: u32,
    pub(super) with_arg: bool,
    pub(super) bindings: [[u32; 3]; 8],
    pub(super) word_count: u32,
    pub(super) word_codes: [u32; 8],
    pub(super) name: [u8; 24],
    pub(super) name_len: u32,
    pub(super) descriptor: [u32; 11],
}
impl NativePage {
    const fn empty() -> Self {
        NativePage {
            draw: 0,
            event: 0,
            flags: 0,
            arg: 0,
            with_arg: false,
            bindings: [[0; 3]; 8],
            word_count: 0,
            word_codes: [0; 8],
            name: [0; 24],
            name_len: 0,
            descriptor: [0; 11],
        }
    }
    pub fn draw(&self) -> u32 {
        self.draw
    }
    pub fn event(&self) -> u32 {
        self.event
    }
    pub fn arg(&self) -> Option<u32> {
        self.with_arg.then_some(self.arg)
    }
    pub fn flags(&self) -> u32 {
        self.flags
    }
    pub fn title(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
    /// The descriptor words; words 0 and 1 hold the name address, which the caller sets in place (`name_ptr`).
    pub fn descriptor(&self) -> [u32; 11] {
        self.descriptor
    }
    pub fn name_ptr(&self) -> *const u8 {
        self.name.as_ptr()
    }
    pub(super) fn level_code(&self) -> u32 {
        self.descriptor[10]
    }
}

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
        bindings: &[[u32; 3]; 8],
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
            || self.pages[ty as usize][index as usize].draw != 0
            || (self.modes[ty as usize] != 0 && self.modes[ty as usize] != mode)
            || words.len() > 8
        {
            return Err(());
        }
        let page = &mut self.pages[ty as usize][index as usize];
        page.name = [0; 24];
        let len = title.iter().take(23).take_while(|&&b| b != 0).count();
        page.name[..len].copy_from_slice(&title[..len]);
        page.name_len = len as u32;
        page.descriptor = [0; 11];
        page.descriptor[10] = LEVEL_CODE; // native track level code, not a new parameter
        page.bindings = *bindings;
        for b in bindings {
            if b[0] == 1 {
                // an encoder position past H would index outside the descriptor: the runtime validates it, the table refuses it
                if b[2] >= 8 {
                    return Err(());
                }
                page.descriptor[2 + b[2] as usize] = b[1];
            }
        }
        page.word_count = words.len() as u32;
        page.word_codes = [0; 8];
        page.word_codes[..words.len()].copy_from_slice(words);
        page.event = event;
        page.flags = flags;
        page.arg = arg;
        page.with_arg = with_arg;
        self.modes[ty as usize] = mode;
        page.draw = draw; // publication last
        self.armed += 1;
        Ok(())
    }
    pub fn remove(&mut self, ty: u32) {
        if ty as usize >= TYPES {
            return;
        }
        for page in &mut self.pages[ty as usize] {
            if page.draw != 0 {
                page.draw = 0;
                page.event = 0;
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
        (p.draw != 0).then_some(p)
    }
    /// The page the view shows, if it is a native one.
    pub fn selected(&self, view: &impl super::view::View) -> Option<&NativePage> {
        let index = view.page_index();
        if !self.armed() || index >= 2 {
            return None;
        }
        let ty = view.native_type();
        if ty < 0 {
            return None;
        }
        self.page(ty as u32, index)
    }
    /// The parameter code of encoder `encoder`: -1 resumes the stock mapper, 0 is a consumed mapping for LOCAL or unused.
    pub fn code(&self, view: &impl super::view::View, encoder: u32) -> i32 {
        let Some(page) = self.selected(view) else {
            return -1;
        };
        if encoder >= 8 {
            return -1;
        }
        page.bindings
            .iter()
            .find(|b| b[0] == 1 && b[2] == encoder)
            .map_or(0, |b| b[1] as i32)
    }
}
