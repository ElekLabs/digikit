//! Fakes shared by the host tests of the native page: the firmware view, the clip surface, the stock canvas and a content module that records what it is given.
use super::*;
use std::cell::RefCell;
use std::vec::Vec;

pub fn fnv(bytes: impl IntoIterator<Item = u8>) -> u32 {
    bytes
        .into_iter()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

pub struct FakeView {
    pub index: u32,
    pub ty: i32,
    pub track: u32,
    values: [(u32, i32); 16],
    pub invalidated: RefCell<u32>,
}
impl FakeView {
    pub fn new() -> Self {
        FakeView {
            index: 1,
            ty: 3,
            track: 0,
            values: [(0, 0); 16],
            invalidated: RefCell::new(0),
        }
    }
    pub fn set(mut self, code: u32, v: i32) -> Self {
        let slot = self
            .values
            .iter_mut()
            .find(|s| s.0 == 0 || s.0 == code)
            .unwrap();
        *slot = (code, v);
        self
    }
}
impl View for FakeView {
    fn page_index(&self) -> u32 {
        self.index
    }
    fn native_type(&self) -> i32 {
        self.ty
    }
    fn track(&self) -> u32 {
        self.track
    }
    fn value(&self, code: u32) -> i32 {
        self.values.iter().find(|s| s.0 == code).map_or(0, |s| s.1)
    }
    fn invalidate(&self) {
        *self.invalidated.borrow_mut() += 1;
    }
}

#[derive(Default)]
pub struct Clip {
    pub fills: Vec<[i32; 5]>,
    pub texts: Vec<(i32, i32, Vec<u8>)>,
}
impl Surface for Clip {
    fn text(&mut self, x: i32, y: i32, text: &[u8]) {
        self.texts.push((x, y, text.to_vec()));
    }
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: i32) {
        self.fills.push([x0, y0, x1, y1, c]);
    }
}
#[derive(Default)]
pub struct Firmware {
    pub fills: Vec<[i32; 5]>,
    pub blits: Vec<(u32, u32, Vec<u32>)>,
}
impl Stock for Firmware {
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: i32) {
        self.fills.push([x0, y0, x1, y1, c]);
    }
    fn blit(&mut self, clipped: &Clipped, origin_x: u32) {
        self.blits
            .push((origin_x, clipped.width(), clipped.plane1().to_vec()));
    }
}

/// A module whose draw callback records the frame it got and then draws.
#[derive(Default)]
pub struct Module {
    pub frames: Vec<Frame>,
    pub events: Vec<(u32, u32, i32, bool)>,
    pub rects: Vec<[i32; 5]>,
}
impl Content<Clip> for Module {
    fn draw(&mut self, _p: &NativePage, f: &Frame, canvas: &mut Canvas<'_, Clip>) {
        self.frames.push(f.clone());
        for r in &self.rects {
            canvas.fill_rect(r[0], r[1], r[2], r[3], r[4]);
        }
    }
    fn event(&mut self, _p: &NativePage, track: u32, control: u32, delta: i32, coarse: bool) {
        self.events.push((track, control, delta, coarse));
    }
}

/// Page of type 3, index 1: PARAM code 40 on encoder H (position 7), LOCAL control 9 on encoder A.
pub fn table(flags: u32, index: u32) -> PageTable {
    let mut t = PageTable::new(0);
    let mut b = [[0u32; 3]; 8];
    b[0] = [2, 9, 0];
    b[1] = [1, 40, 7];
    t.register(
        3,
        index,
        2,
        flags,
        b"Title",
        0x1000,
        0x2000,
        &b,
        &[41, 42],
        0,
        false,
    )
    .unwrap();
    t
}
