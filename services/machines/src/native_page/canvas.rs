use super::surface::Surface;

pub const WIDTH: u32 = 99;
pub const WIDE_WIDTH: u32 = 107;
pub const HEIGHT: i32 = 53;
pub const ORIGIN_X: u32 = 25;
/// Words of one plane at the widest canvas: `width * ceil(height / 32) * 4`.
pub const PLANE_WORDS: usize = WIDE_WIDTH as usize * 2 * 4;
pub const MASK_WORDS: usize = WIDE_WIDTH as usize * 2;

/// The two working planes of the clipped bitmap. Plane 1 is the write mask of the blit: it starts opaque and keeps the rounded-up band transparent.
pub struct Clipped {
    pub width: u32,
    pub plane0: [u32; PLANE_WORDS],
    pub plane1: [u32; PLANE_WORDS],
}
impl Clipped {
    pub fn new(width: u32) -> Self {
        let mut c = Clipped {
            width,
            plane0: [0; PLANE_WORDS],
            plane1: [0; PLANE_WORDS],
        };
        for x in 0..width as usize {
            c.plane1[2 * x] = u32::MAX;
            c.plane1[2 * x + 1] = u32::MAX << (64 - HEIGHT as u32);
        }
        c
    }
}

/// Logical mask of the overlay: x-major bands, the MSB is row 0 of the bottom-origin band.
pub fn mask_rect(mask: &mut [u32], width: i32, height: i32, x0: i32, y0: i32, x1: i32, y1: i32) {
    let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(width - 1), y1.min(height - 1));
    let bands = (height + 31) / 32;
    for x in x0..=x1 {
        for y in y0..=y1 {
            let row = height - 1 - y;
            mask[(x * bands + row / 32) as usize] |= 1u32 << (31 - row % 32);
        }
    }
}

/// What the content callback draws on: top-origin local coordinates, clipped to the page canvas, drawn into the bitmap bottom-origin.
pub struct Canvas<'a, S: Surface> {
    pub(super) surface: &'a mut S,
    pub(super) width: u32,
    pub(super) overlay: bool,
    pub(super) mask: &'a mut [u32; MASK_WORDS],
}
impl<S: Surface> Canvas<'_, S> {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn text(&mut self, x: i32, y: i32, text: &[u8]) {
        if x < 0 || x >= self.width as i32 || !(0..HEIGHT).contains(&y) {
            return;
        }
        let end = text
            .iter()
            .take(32)
            .position(|&b| b == 0)
            .unwrap_or(text.len().min(32));
        self.surface.text(x, HEIGHT - 1 - y, &text[..end]);
    }
    pub fn fill_rect(&mut self, mut x0: i32, mut y0: i32, mut x1: i32, mut y1: i32, color: i32) {
        if x0 > x1 {
            core::mem::swap(&mut x0, &mut x1);
        }
        if y0 > y1 {
            core::mem::swap(&mut y0, &mut y1);
        }
        let w = self.width as i32;
        if x1 < 0 || y1 < 0 || x0 >= w || y0 >= HEIGHT {
            return;
        }
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w - 1), y1.min(HEIGHT - 1));
        if self.overlay {
            mask_rect(self.mask, w, HEIGHT, x0, y0, x1, y1);
        }
        self.surface.fill_rect(
            x0,
            HEIGHT - 1 - y1,
            x1,
            HEIGHT - 1 - y0,
            (color != 0) as i32,
        );
    }
}
