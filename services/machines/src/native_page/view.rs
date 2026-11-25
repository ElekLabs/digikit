use super::canvas::Canvas;
use super::frame::Frame;
use super::page::NativePage;
use super::surface::Surface;

/// The firmware view as the page logic needs it. The service implements it over the view's fields (`+0x90` page index, `+0x68`, `+0x74`, `+0x78`) and its
/// vtable (`+0x54` value reader, invalidate).
pub trait View {
    /// The selected native page index (word at `+0x90`).
    fn page_index(&self) -> u32;
    /// The machine type of the selected sound (`mf_native_type`).
    fn native_type(&self) -> i32;
    /// The selected track.
    fn track(&self) -> u32;
    /// A parameter value by code, read with the "current" selector (-1).
    fn value(&self, code: u32) -> i32;
    fn invalidate(&self);
}

impl<V: View + ?Sized> View for &V {
    fn page_index(&self) -> u32 {
        (**self).page_index()
    }
    fn native_type(&self) -> i32 {
        (**self).native_type()
    }
    fn track(&self) -> u32 {
        (**self).track()
    }
    fn value(&self, code: u32) -> i32 {
        (**self).value(code)
    }
    fn invalidate(&self) {
        (**self).invalidate()
    }
}

/// The module's callbacks of a page: draw gets the frame and a canvas, event gets the local control event.
pub trait Content<S: Surface> {
    fn draw(&mut self, page: &NativePage, frame: &Frame, canvas: &mut Canvas<'_, S>);
    fn event(&mut self, page: &NativePage, track: u32, control: u32, delta: i32, coarse: bool);
}
