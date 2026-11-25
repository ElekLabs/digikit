/// The stock draw routines of the firmware over a bitmap. Rows are bottom-origin, as the firmware counts them.
pub trait Surface {
    /// Text at (x, y) in the native font, `text` already bounded to 32 bytes.
    fn text(&mut self, x: i32, y: i32, text: &[u8]);
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
}

/// The firmware canvas the clipped bitmap is composited onto.
pub trait Stock {
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
    /// Blits the clipped bitmap with its plane 1 as write mask at `origin_x`.
    fn blit(&mut self, clipped: &super::canvas::Clipped, origin_x: u32);
}
