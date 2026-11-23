//! Host tests of the native page logic. The cases are those of the Python `test_native_page_frame.py`: the frame the content sees (level, parameters, bounds), the
//! page dots and the overlay mask. Golden values for dots and mask are the FNV-1a of what the C headers `native-page-dots.h` and `native-page-mask.h` produce.
use super::canvas::mask_rect;
use super::paint::frame_of;
use super::*;
use std::cell::RefCell;
use std::vec::Vec;

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u32 {
    bytes
        .into_iter()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

struct FakeView {
    index: u32,
    ty: i32,
    track: u32,
    values: [(u32, i32); 16],
    invalidated: RefCell<u32>,
}
impl FakeView {
    fn new() -> Self {
        FakeView {
            index: 1,
            ty: 3,
            track: 0,
            values: [(0, 0); 16],
            invalidated: RefCell::new(0),
        }
    }
    fn set(mut self, code: u32, v: i32) -> Self {
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
struct Clip {
    fills: Vec<[i32; 5]>,
    texts: Vec<(i32, i32, Vec<u8>)>,
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
struct Firmware {
    fills: Vec<[i32; 5]>,
    blits: Vec<(u32, u32, Vec<u32>)>,
}
impl Stock for Firmware {
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: i32) {
        self.fills.push([x0, y0, x1, y1, c]);
    }
    fn blit(&mut self, clipped: &Clipped, origin_x: u32) {
        self.blits
            .push((origin_x, clipped.width, clipped.plane1.to_vec()));
    }
}

/// A module whose draw callback records the frame it got and then draws.
#[derive(Default)]
struct Module {
    frames: Vec<Frame>,
    events: Vec<(u32, u32, i32, bool)>,
    rects: Vec<[i32; 5]>,
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
fn table(flags: u32, index: u32) -> PageTable {
    let mut t = PageTable::new();
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

#[test]
fn frame_reports_param_and_level_independently() {
    let t = table(0, 1);
    let page = t.page(3, 1).unwrap();
    let v = FakeView::new()
        .set(40, 112 << 8)
        .set(10, 12345)
        .set(41, 5)
        .set(42, -6);
    let f = frame_of(&v, page, 0);
    assert_eq!((f.params[7], f.level, f.word_count), (112 << 8, 12345, 2));
    assert_eq!(f.word_values, [5, -6, 0, 0, 0, 0, 0, 0]);
    let v = FakeView::new().set(40, 122 << 8).set(10, 12345);
    let g = frame_of(&v, page, 0);
    assert_eq!((g.params[7], g.level), (122 << 8, 12345));
    let v = FakeView::new().set(40, 122 << 8).set(10, 6789);
    assert_eq!(frame_of(&v, page, 0).level, 6789);
}

#[test]
fn frame_bytes_match_the_c_layout() {
    let t = table(0, 1);
    let v = FakeView::new().set(40, 112 << 8).set(10, 6789);
    let bytes = frame_of(&v, t.page(3, 1).unwrap(), 5).to_bytes(0xAA, 0xBB);
    let mut want = Vec::new();
    for w in [
        1u32,
        100,
        5,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        112 << 8,
        99,
        53,
        0xAA,
        0xBB,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        6789,
    ] {
        want.extend_from_slice(&w.to_be_bytes());
    }
    assert_eq!(bytes.to_vec(), want);
    // the level is the last word: bytes 24 and 25 of the C test's status row are the high half
    assert_eq!(
        &bytes[bytes.len() - 4..],
        &[0, 0, (6789 >> 8) as u8, (6789 & 255) as u8]
    );
}

#[test]
fn draw_hands_content_the_frame_and_refuses_bad_tracks() {
    let t = table(0, 1);
    let mut v = FakeView::new().set(40, 112 << 8);
    let (mut clip, mut fw, mut m) = (Clip::default(), Firmware::default(), Module::default());
    assert_eq!(draw(&t, &v, &mut clip, &mut fw, &mut m), Draw::Handled);
    assert_eq!(m.frames.len(), 1);
    v.track = 16;
    assert_eq!(draw(&t, &v, &mut clip, &mut fw, &mut m), Draw::Stock);
    assert_eq!(m.frames.len(), 1);
    v.track = 0;
    v.ty = 4; // another machine type: stock
    assert_eq!(draw(&t, &v, &mut clip, &mut fw, &mut m), Draw::Stock);
}

#[test]
fn draw_composites_the_clipped_bitmap_and_page_dots() {
    let t = table(0, 1);
    let v = FakeView::new();
    let (mut clip, mut fw, mut m) = (Clip::default(), Firmware::default(), Module::default());
    draw(&t, &v, &mut clip, &mut fw, &mut m);
    assert_eq!(fw.fills[0], [25, 0, 127, 52, 0]);
    let (x, w, plane1) = &fw.blits[0];
    assert_eq!((*x, *w), (25, 99));
    assert_eq!(plane1[0], u32::MAX);
    assert_eq!(plane1[1], u32::MAX << 11);
    assert_eq!(
        &fw.fills[1..],
        &[[125, 32, 126, 33, -1], [124, 23, 127, 26, -1]]
    );
}

#[test]
fn wide_canvas_moves_the_origin_left() {
    let t = table(FLAG_WIDE_CANVAS, 1);
    let (mut clip, mut fw, mut m) = (Clip::default(), Firmware::default(), Module::default());
    draw(&t, &FakeView::new(), &mut clip, &mut fw, &mut m);
    assert_eq!((fw.blits[0].0, fw.blits[0].1), (17, 107));
    assert_eq!(m.frames[0].width, 107);
}

#[test]
fn overlay_page_draws_in_two_steps_and_masks_only_filled_rectangles() {
    let t = table(FLAG_STOCK_OVERLAY, 0);
    let mut v = FakeView::new();
    v.index = 0;
    let (mut clip, mut fw, mut m) = (Clip::default(), Firmware::default(), Module::default());
    m.rects = std::vec![[0, 0, 22, 19, 1]];
    assert_eq!(
        draw(&t, &v, &mut clip, &mut fw, &mut m),
        Draw::StockThenOverlay
    );
    assert!(m.frames.is_empty() && fw.blits.is_empty());
    overlay(&t, &v, &mut clip, &mut fw, &mut m);
    assert_eq!(m.frames.len(), 1);
    assert!(
        fw.fills.is_empty(),
        "an overlay clears nothing and draws no dots"
    );
    let plane1 = &fw.blits[0].2;
    // x = 0, rows y 0..=19 are the bottom-origin rows 33..=52: band 1, bits 30..=11
    assert_eq!(plane1[0], 0);
    assert_eq!(plane1[1], 0x7fff_f800);
    assert_eq!(plane1[2 * 23], 0);
    // revalidation: the view changed under the stock draw
    v.ty = 9;
    let before = m.frames.len();
    overlay(&t, &v, &mut clip, &mut fw, &mut m);
    assert_eq!(m.frames.len(), before);
}

#[test]
fn canvas_clips_and_flips_to_the_bitmap() {
    let mut clip = Clip::default();
    let mut mask = [0u32; MASK_WORDS];
    let mut c = Canvas {
        surface: &mut clip,
        width: 99,
        overlay: false,
        mask: &mut mask,
    };
    c.fill_rect(10, 20, 5, 2, 1); // swapped corners
    c.fill_rect(-5, -5, 200, 100, 7); // clamps, color becomes 1
    c.fill_rect(99, 0, 120, 5, 1); // outside
    c.fill_rect(0, 53, 5, 60, 1); // outside
    c.text(-1, 0, b"x");
    c.text(0, 53, b"x");
    c.text(3, 4, b"hello\0world");
    assert_eq!(clip.fills, [[5, 52 - 20, 10, 52 - 2, 1], [0, 0, 98, 52, 1]]);
    assert_eq!(clip.texts, [(3, 48, b"hello".to_vec())]);
}

#[test]
fn dots_match_the_c_header() {
    let golden = [0x62b95591u32, 0x00f09591, 0xbcc31dc5];
    for (page, want) in golden.iter().enumerate() {
        let mut pix = [[0u8; 128]; 64];
        dots(page as u32, |x0, y0, x1, y1, c| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    pix[y as usize][x as usize] = (c != 0) as u8;
                }
            }
        });
        assert_eq!(fnv(pix.iter().flatten().copied()), *want, "page {page}");
    }
    // the C test: page 0 is the upper dot, page 1 the lower, page 2 nothing
    let count = |page: u32, first: i32, last: i32| {
        let mut n = 0;
        dots(page, |x0, y0, x1, y1, _| {
            for y in y0..=y1 {
                n += (x1 - x0 + 1) * (first..=last).contains(&(63 - y)) as i32;
            }
        });
        n
    };
    assert!(count(0, 28, 33) > count(0, 35, 41));
    assert!(count(1, 28, 33) < count(1, 35, 41));
    assert_eq!(count(2, 0, 63), 0);
}

#[test]
fn overlay_mask_matches_the_c_header() {
    let hash = |m: &[u32]| fnv(m.iter().flat_map(|w| w.to_le_bytes()));
    let mut mask = [0u32; 99 * 2];
    for (a, b) in [(0, 22), (24, 47), (76, 98)] {
        mask_rect(&mut mask, 99, 53, a, 0, b, 19);
    }
    assert_eq!(hash(&mask), 0x459438c9);
    let visible = |x: usize, y: usize| {
        let row = 52 - y;
        (mask[x * 2 + row / 32] >> (31 - row % 32)) & 1
    };
    for x in 0..99usize {
        for y in 0..53usize {
            let want = y <= 19 && (x <= 22 || (24..=47).contains(&x) || x >= 76);
            assert_eq!(visible(x, y) == 1, want, "({x},{y})");
        }
    }
    mask_rect(&mut mask, 99, 53, -10, -10, 200, 100);
    assert_eq!(hash(&mask), 0xcc2e4403);
    for x in 0..99 {
        assert_eq!((mask[x * 2], mask[x * 2 + 1]), (u32::MAX, u32::MAX << 11));
    }
}

#[test]
fn registration_rules() {
    let b = [[0u32; 3]; 8];
    let mut t = PageTable::new();
    let reg = |t: &mut PageTable, ty, idx, mode, flags, draw, words: &[u32]| {
        t.register(ty, idx, mode, flags, b"T", draw, 1, &b, words, 0, false)
    };
    assert!(reg(&mut t, 32, 1, 1, 0, 1, &[]).is_err(), "type bound");
    assert!(reg(&mut t, 0, 2, 1, 0, 1, &[]).is_err(), "index bound");
    assert!(reg(&mut t, 0, 1, 1, 0, 0, &[]).is_err(), "draw required");
    assert!(
        reg(&mut t, 0, 1, 1, FLAG_WIDE_CANVAS, 1, &[]).is_err(),
        "flags need v2"
    );
    assert!(
        reg(&mut t, 0, 1, 2, FLAG_STOCK_OVERLAY, 1, &[]).is_err(),
        "overlay is page 0"
    );
    assert!(
        reg(&mut t, 0, 0, 2, FLAG_WIDE_CANVAS, 1, &[]).is_err(),
        "wide is page 1"
    );
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[0; 9]).is_err(), "word bound");
    assert!(!t.armed());
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[]).is_ok());
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[]).is_err(), "twice");
    assert!(reg(&mut t, 0, 0, 2, 0, 1, &[]).is_err(), "modes do not mix");
    assert_eq!(t.page_count(0, 3), 4, "v1 appends");
    assert_eq!(t.page_count(5, 3), 3);
    t.remove(0);
    assert!(!t.armed() && t.page(0, 1).is_none());
    assert!(reg(&mut t, 0, 0, 2, 0, 1, &[]).is_ok());
    assert_eq!(t.page_count(0, 3), 2, "v2 replaces");
}

#[test]
fn registration_builds_name_and_descriptor() {
    let mut b = [[0u32; 3]; 8];
    b[2] = [1, 77, 2];
    b[3] = [2, 5, 3];
    let mut t = PageTable::new();
    t.register(
        1,
        1,
        1,
        0,
        b"A very long title that is cut at 23",
        1,
        1,
        &b,
        &[],
        0,
        false,
    )
    .unwrap();
    let p = t.page(1, 1).unwrap();
    assert_eq!(p.title(), b"A very long title that ");
    let d = p.descriptor();
    assert_eq!(d[2 + 2], 77);
    assert_eq!(d[10], 10);
    assert_eq!(
        (d[5], d[0]),
        (0, 0),
        "a LOCAL control has no descriptor code"
    );
    let mut bad = b;
    bad[0] = [1, 1, 8];
    assert!(t
        .register(2, 1, 1, 0, b"x", 1, 1, &bad, &[], 0, false)
        .is_err());
}

#[test]
fn code_maps_encoders_for_native_pages_only() {
    let t = table(0, 1);
    let v = FakeView::new();
    assert_eq!(t.code(&v, 7), 40);
    assert_eq!(t.code(&v, 0), 0, "LOCAL is consumed");
    assert_eq!(t.code(&v, 4), 0, "unused is consumed");
    assert_eq!(t.code(&v, 8), -1);
    let mut other = FakeView::new();
    other.ty = 4;
    assert_eq!(t.code(&other, 7), -1);
    other.ty = -1;
    assert_eq!(t.code(&other, 7), -1);
    assert_eq!(PageTable::new().code(&v, 7), -1);
}

#[test]
fn turn_dispatch() {
    let t = table(0, 1);
    let v = FakeView::new();
    let mut m = Module::default();
    let ev = |control, delta, coarse| Event {
        control,
        delta,
        coarse,
    };
    assert_eq!(
        turn(&t, &v, &ev(8, 1, false), &mut m),
        0,
        "PARAM edit runs stock"
    );
    assert_eq!(turn(&t, &v, &ev(1, -2, true), &mut m), 1);
    assert_eq!(m.events, [(0, 9, -2, true)]);
    assert_eq!(*v.invalidated.borrow(), 1);
    assert_eq!(
        turn(&t, &v, &ev(5, 1, false), &mut m),
        1,
        "unused is consumed"
    );
    assert_eq!(m.events.len(), 1);
    assert_eq!(turn(&t, &v, &ev(0, 1, false), &mut m), 0);
    assert_eq!(turn(&t, &v, &ev(9, 1, false), &mut m), 0);
    let mut other = FakeView::new();
    other.index = 0;
    assert_eq!(
        turn(&t, &other, &ev(1, 1, false), &mut m),
        0,
        "not a native page"
    );
}
