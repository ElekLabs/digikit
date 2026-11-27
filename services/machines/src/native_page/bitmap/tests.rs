//! The bitmap header the stock draw routines read.
use super::*;
use core::mem::{offset_of, size_of};

#[test]
fn the_header_has_the_firmware_layout_and_points_at_the_planes() {
    assert_eq!(size_of::<Bitmap>(), BITMAP_BYTES);
    assert_eq!(offset_of!(Bitmap, width), OFF_WIDTH);
    assert_eq!(offset_of!(Bitmap, height), OFF_HEIGHT);
    assert_eq!(offset_of!(Bitmap, bands), OFF_BANDS);
    assert_eq!(offset_of!(Bitmap, plane0), OFF_PLANE0);
    assert_eq!(offset_of!(Bitmap, plane1), OFF_PLANE1);
    let mut c = Clipped::new(107);
    let [p0, p1] = c.plane_ptrs();
    let b = Bitmap::over(&mut c, 0x4010_0000);
    assert_eq!(
        (b.vptr, b.width, b.height, b.bands),
        (0x4010_0000, 107, 53, 2)
    );
    assert_eq!(
        (b.plane0, b.plane1),
        (p0 as usize as u32, p1 as usize as u32)
    );
}
