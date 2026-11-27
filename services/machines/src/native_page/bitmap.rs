use super::canvas::{Clipped, HEIGHT};

/// The firmware's `Bitmap` as the stock draw routines read it: the vtable of the screen bitmap (the blit takes its row routine from it), width, height, the 32-row bands per column
/// and the two planes. Words are 32 bits as on the ColdFire; the service file gives the planes' addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Bitmap {
    vptr: u32,
    width: u32,
    height: u32,
    bands: u32,
    plane0: u32,
    plane1: u32,
}

#[cfg(test)]
pub const OFF_WIDTH: usize = 4;
#[cfg(test)]
pub const OFF_HEIGHT: usize = 8;
#[cfg(test)]
pub const OFF_BANDS: usize = 12;
#[cfg(test)]
pub const OFF_PLANE0: usize = 16;
#[cfg(test)]
pub const OFF_PLANE1: usize = 20;
#[cfg(test)]
pub const BITMAP_BYTES: usize = 24;

impl Bitmap {
    pub const fn empty() -> Self {
        Bitmap {
            vptr: 0,
            width: 0,
            height: 0,
            bands: 0,
            plane0: 0,
            plane1: 0,
        }
    }
    /// The bitmap over the planes of `clipped`, with the vtable of the firmware canvas it is blitted onto.
    pub fn over(clipped: &mut Clipped, vptr: u32) -> Self {
        let [p0, p1] = clipped.plane_ptrs();
        Bitmap {
            vptr,
            width: clipped.width(),
            height: HEIGHT as u32,
            bands: 2,
            plane0: p0 as usize as u32,
            plane1: p1 as usize as u32,
        }
    }
    pub fn width(&self) -> u32 {
        self.width
    }
}

#[cfg(test)]
mod tests;
