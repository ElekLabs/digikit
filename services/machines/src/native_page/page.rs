pub const FLAG_STOCK_OVERLAY: u32 = 1;
pub const FLAG_WIDE_CANVAS: u32 = 2;
/// The native track level code, not a new parameter.
const LEVEL_CODE: u32 = 10;
/// Encoder positions A to H.
pub const ENCODERS: usize = 8;

/// What a page is built from; the table has checked it.
pub(super) struct PageSpec<'a> {
    pub title: &'a [u8],
    pub flags: u32,
    pub draw: u32,
    pub event: u32,
    pub bindings: &'a [[u32; 3]; ENCODERS],
    pub words: &'a [u32],
    pub arg: u32,
    pub with_arg: bool,
}

/// A `std::string` representation as the firmware reads it: the characters are preceded by length, capacity and a reference count of -1 (a leaked string that nobody shares or frees).
#[derive(Clone, Copy)]
#[repr(C)]
struct Rep {
    len: u32,
    cap: u32,
    refs: i32,
    text: [u8; 24],
}
impl Rep {
    const fn empty() -> Self {
        Rep {
            len: 0,
            cap: 0,
            refs: 0,
            text: [0; 24],
        }
    }
}

/// One registered page. The binding rows are (kind, code or local id, encoder position): kind 1 is a PARAM, kind 2 a LOCAL control, 0 unused.
#[derive(Clone)]
pub struct NativePage {
    draw: u32,
    event: u32,
    flags: u32,
    arg: u32,
    with_arg: bool,
    bindings: [[u32; 3]; ENCODERS],
    word_count: u32,
    word_codes: [u32; 8],
    name: Rep,
    descriptor: [u32; 11],
}
impl NativePage {
    pub(super) const fn empty() -> Self {
        NativePage {
            draw: 0,
            event: 0,
            flags: 0,
            arg: 0,
            with_arg: false,
            bindings: [[0; 3]; ENCODERS],
            word_count: 0,
            word_codes: [0; 8],
            name: Rep::empty(),
            descriptor: [0; 11],
        }
    }
    /// A live page from a checked spec: the title cut at 23 bytes or a NUL, the descriptor with the PARAM codes at their encoder positions and the level code last.
    pub(super) fn build(spec: &PageSpec) -> Self {
        let mut page = Self::empty();
        let len = spec.title.iter().take(23).take_while(|&&b| b != 0).count();
        page.name.text[..len].copy_from_slice(&spec.title[..len]);
        page.name.len = len as u32;
        page.name.cap = 23;
        page.name.refs = -1;
        page.descriptor[10] = LEVEL_CODE;
        page.bindings = *spec.bindings;
        for b in spec.bindings.iter().filter(|b| b[0] == 1) {
            page.descriptor[2 + b[2] as usize] = b[1];
        }
        page.word_count = spec.words.len() as u32;
        page.word_codes[..spec.words.len()].copy_from_slice(spec.words);
        page.event = spec.event;
        page.flags = spec.flags;
        page.arg = spec.arg;
        page.with_arg = spec.with_arg;
        page.draw = spec.draw;
        page
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
        &self.name.text[..self.name.len as usize]
    }
    /// The descriptor words; words 0 and 1 hold the address of the name's characters once the page sits in its table (`anchor`).
    pub fn descriptor(&self) -> [u32; 11] {
        self.descriptor
    }
    /// The characters of the name, preceded by their `std::string` representation.
    pub fn name_ptr(&self) -> *const u8 {
        self.name.text.as_ptr()
    }
    /// The descriptor the firmware reads, in place: the address stays while the page stays in its table.
    pub fn descriptor_ptr(&self) -> *const u32 {
        self.descriptor.as_ptr()
    }
    /// Points descriptor words 0 and 1 (long and short name) at this page's own name. Called once the page is at its final address: the table.
    pub(super) fn anchor(&mut self) {
        let at = self.name.text.as_ptr() as usize as u32;
        self.descriptor[0] = at;
        self.descriptor[1] = at;
    }
    pub fn bindings(&self) -> &[[u32; 3]; ENCODERS] {
        &self.bindings
    }
    pub fn word_codes(&self) -> &[u32] {
        &self.word_codes[..self.word_count as usize]
    }
    pub fn level_code(&self) -> u32 {
        self.descriptor[10]
    }
}
