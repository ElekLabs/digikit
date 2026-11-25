//! The native pages of the machine page, served by the file (RC4-3, D8): the hooks of the machine page call the entries `dks_native_*` below, which look the page up in the owner's
//! `PageTable` and run the logic of `native_page/` over the firmware. This file is the firmware's side only: the view's fields and vtable, the stock draw routines on the bitmaps,
//! the content's callbacks. The page table is the owner's; a page is cloned out of it before its callbacks run, because a callback may call back into the file.
//!
//! One UI task draws and turns, so the clipped bitmap, its header and the canvas the content draws on are statics (as in the image's adapter: the planes are 6.8 KiB, too big
//! for a UI stack). A host build has no firmware: the firmware calls are no-ops.
use crate::fw::DkFw;
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::{addr_of, addr_of_mut, null_mut, read_volatile};
use dkrt_abi::fw::Fw;
use dkrt_svc_machines::info::{w, DKRT_MF_TAB};
use dkrt_svc_machines::native_page::{
    draw_page, overlay_page, turn_page, Bitmap, Canvas, Clipped, Content, Draw, Event, Frame,
    NativePage, PageEvent, Stock, Surface, View, PAGES,
};
use dkrt_svc_rt::Outside;

/// Offsets in the machine page's view (the same on both devices): the selected page, the browser's sounds, the track reference and the sounds of the kit.
const VIEW_PAGE: usize = 0x90;
const VIEW_LIBRARY: usize = 0x68;
const VIEW_TRACK_REF: usize = 0x74;
const VIEW_SOUNDS: usize = 0x78;
/// Offsets in the encoder event: the 1-based encoder, the detents and the coarse byte.
const EV_CONTROL: usize = 0xc;
const EV_DELTA: usize = 0x10;
const EV_COARSE: usize = 0x14;

unsafe fn word(base: *const u8, off: usize) -> u32 {
    read_volatile(base.add(off) as *const u32)
}

/// The firmware calls the native pages make. Each is a no-op with the neutral answer where the firmware has no such function (address 0, `guard.rs`) and on the host.
#[cfg(target_arch = "m68k")]
mod fwcall {
    use crate::guard::call_if_bound;

    pub unsafe fn machine_type_slotmap(view: *const u8) -> i32 {
        call_if_bound(dkfw::machine::current_track_type as usize, -1, || {
            dkfw::machine::current_track_type(view as *mut u8)
        })
    }
    pub unsafe fn sound_index(view: *const u8) -> u32 {
        call_if_bound(dkfw::page::native_sound_index as usize, 0, || {
            dkfw::page::native_sound_index(view as *mut u8)
        })
    }
    pub unsafe fn library_sound(sounds: *const u8, index: u32) -> *const u8 {
        call_if_bound(
            dkfw::page::native_library_sound as usize,
            core::ptr::null(),
            || dkfw::page::native_library_sound(sounds as *mut u8, index) as *const u8,
        )
    }
    pub unsafe fn track_sound(sounds: *const u8, track: u32) -> *const u8 {
        call_if_bound(
            dkfw::page::native_track_sound as usize,
            core::ptr::null(),
            || dkfw::page::native_track_sound(sounds as *mut u8, track) as *const u8,
        )
    }
    pub unsafe fn invalidate(view: *const u8) {
        call_if_bound(dkfw::view::invalidate as usize, (), || {
            dkfw::view::invalidate(view as *mut u8)
        })
    }
    pub unsafe fn fill_rect(bitmap: *mut u8, x0: i32, y0: i32, x1: i32, y1: i32, color: i32) {
        call_if_bound(dkfw::draw::fill_rect as usize, (), || {
            dkfw::draw::fill_rect(bitmap, x0, y0, x1, y1, color)
        })
    }
    pub unsafe fn blit(dst: *mut u8, src: *mut u8, x: i32) {
        call_if_bound(dkfw::draw::blit as usize, (), || {
            dkfw::draw::blit(dst, src, x, 0, 0)
        })
    }
    /// `%s` of `text` (NUL-terminated) in the native font at (x, y).
    pub unsafe fn text(bitmap: *mut u8, x: i32, y: i32, text: *const u8) {
        let font = core::ptr::read_volatile(
            core::ptr::addr_of!(dkfw::page::NATIVE_FONT_OPERAND) as *const u32
        ) as *mut u8;
        call_if_bound(dkfw::text::draw_fmt as usize, (), || {
            dkfw::text::draw_fmt(bitmap, font, x, y, 0, b"%s\0".as_ptr(), text);
        })
    }
}
#[cfg(not(target_arch = "m68k"))]
mod fwcall {
    pub unsafe fn machine_type_slotmap(_: *const u8) -> i32 {
        -1
    }
    pub unsafe fn sound_index(_: *const u8) -> u32 {
        0
    }
    pub unsafe fn library_sound(_: *const u8, _: u32) -> *const u8 {
        core::ptr::null()
    }
    pub unsafe fn track_sound(_: *const u8, _: u32) -> *const u8 {
        core::ptr::null()
    }
    pub unsafe fn invalidate(_: *const u8) {}
    pub unsafe fn fill_rect(_: *mut u8, _: i32, _: i32, _: i32, _: i32, _: i32) {}
    pub unsafe fn blit(_: *mut u8, _: *mut u8, _: i32) {}
    pub unsafe fn text(_: *mut u8, _: i32, _: i32, _: *const u8) {}
}

/// The machine type of the selected sound of the view, as the image's `mf_native_type` gave it. A device whose type reader is hooked (the image gives the copy of the stock
/// reader, `READER_REAL`) takes the sound of the browser or the track and reads its type with that copy; the other asks the firmware's track type function.
unsafe fn machine_type(view: *const u8, fw: &DkFw) -> i32 {
    let reader = read_volatile((addr_of!(DKRT_MF_TAB) as *const u32).add(w::READER_REAL));
    if reader == 0 {
        return fwcall::machine_type_slotmap(view);
    }
    let index = fwcall::sound_index(view);
    let sound = if index < 0x80 {
        fwcall::library_sound(word(view, VIEW_LIBRARY) as *const u8, index)
    } else {
        let track = fw.view_track(word(view, VIEW_TRACK_REF) as usize);
        fwcall::track_sound(word(view, VIEW_SOUNDS) as *const u8, track)
    };
    let read: unsafe extern "C" fn(*const u8) -> u32 = transmute(reader as usize);
    read(sound) as i32
}

/// The firmware view of the machine page, with the page's firmware handle: made only from the [`Outside`] token (`with_view`), so its reads never run inside the owner.
struct FwView(*const u8, DkFw);
impl View for FwView {
    fn page_index(&self) -> u32 {
        unsafe { word(self.0, VIEW_PAGE) }
    }
    fn native_type(&self) -> i32 {
        unsafe { machine_type(self.0, &self.1) }
    }
    fn track(&self) -> u32 {
        unsafe { self.1.view_track(word(self.0, VIEW_TRACK_REF) as usize) }
    }
    fn value(&self, code: u32) -> i32 {
        self.1.page_value(self.0 as usize, code)
    }
    fn invalidate(&self) {
        unsafe { fwcall::invalidate(self.0) }
    }
}

// The clipped bitmap the content draws on and the header the stock draw routines read it by (statics: see the module doc).
static mut CLIPPED: Clipped = Clipped::empty();
static mut BITMAP: Bitmap = Bitmap::empty();
/// The canvas the content's `text` and `fill_rect` callbacks reach (they take no context): set for the length of one draw callback.
static mut CURRENT: *mut c_void = null_mut();

/// The header of the clipped bitmap, current for the page's width (`reset` sets it after the header was made, so every call makes it again).
unsafe fn bitmap(vptr: u32) -> *mut u8 {
    *addr_of_mut!(BITMAP) = Bitmap::over(&mut *addr_of_mut!(CLIPPED), vptr);
    addr_of_mut!(BITMAP) as *mut u8
}

/// What the content draws on: the stock text and rectangle routines over the clipped bitmap.
struct FwSurface {
    vptr: u32,
}
impl Surface for FwSurface {
    fn text(&mut self, x: i32, y: i32, text: &[u8]) {
        // bounded to 32 bytes by the canvas; the stock routine wants a C string
        let mut c = [0u8; 33];
        c[..text.len().min(32)].copy_from_slice(&text[..text.len().min(32)]);
        unsafe { fwcall::text(bitmap(self.vptr), x, y, c.as_ptr()) }
    }
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: i32) {
        unsafe { fwcall::fill_rect(bitmap(self.vptr), x0, y0, x1, y1, color) }
    }
}

/// The firmware canvas the clipped bitmap is composited onto.
struct FwStock {
    canvas: *mut u8,
    vptr: u32,
}
impl Stock for FwStock {
    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: i32) {
        unsafe { fwcall::fill_rect(self.canvas, x0, y0, x1, y1, color) }
    }
    fn blit(&mut self, clipped: &Clipped, origin_x: u32) {
        // the planes are the static `CLIPPED`, which `clipped` is
        debug_assert!(core::ptr::eq(clipped, addr_of!(CLIPPED)));
        unsafe { fwcall::blit(self.canvas, bitmap(self.vptr), origin_x as i32) }
    }
}

/// The frame the content's draw callback reads, on a word boundary.
#[repr(C, align(4))]
struct FrameBytes([u8; dkrt_svc_machines::native_page::FRAME_BYTES]);

unsafe extern "C" fn canvas_text(x: i32, y: i32, text: *const u8) {
    let Some(canvas) = (CURRENT as *mut Canvas<'static, FwSurface>).as_mut() else {
        return;
    };
    if text.is_null() {
        return;
    }
    let mut n = 0;
    while n < 32 && *text.add(n) != 0 {
        n += 1;
    }
    canvas.text(x, y, core::slice::from_raw_parts(text, n));
}

unsafe extern "C" fn canvas_fill(x0: i32, y0: i32, x1: i32, y1: i32, color: i32) {
    if let Some(canvas) = (CURRENT as *mut Canvas<'static, FwSurface>).as_mut() {
        canvas.fill_rect(x0, y0, x1, y1, color);
    }
}

/// The callbacks of a module's page: `draw(arg, frame)` and `event(arg, event)`, or without `arg` for a page that has none.
struct FwContent;
impl Content<FwSurface> for FwContent {
    fn draw(&mut self, page: &NativePage, frame: &Frame, canvas: &mut Canvas<'_, FwSurface>) {
        let bytes =
            FrameBytes(frame.to_bytes(canvas_text as usize as u32, canvas_fill as usize as u32));
        unsafe {
            CURRENT = canvas as *mut Canvas<'_, FwSurface> as *mut c_void;
            match page.arg() {
                Some(arg) => {
                    let f: unsafe extern "C" fn(u32, *const u8) = transmute(page.draw() as usize);
                    f(arg, bytes.0.as_ptr());
                }
                None => {
                    let f: unsafe extern "C" fn(*const u8) = transmute(page.draw() as usize);
                    f(bytes.0.as_ptr());
                }
            }
            CURRENT = null_mut();
        }
    }
    fn event(&mut self, page: &NativePage, track: u32, control: u32, delta: i32, coarse: bool) {
        let ev = PageEvent::new(track, control, delta, coarse);
        let at = &ev as *const PageEvent as *const u8;
        unsafe {
            match page.arg() {
                Some(arg) => {
                    let f: unsafe extern "C" fn(u32, *const u8) = transmute(page.event() as usize);
                    f(arg, at);
                }
                None => {
                    let f: unsafe extern "C" fn(*const u8) = transmute(page.event() as usize);
                    f(at);
                }
            }
        }
    }
}

/// Runs `f` on the firmware view of `view` outside the owner (the `act` phase of `Owner::step`): the view's reads are firmware calls, and a stock getter may enter the owner
/// again through a hook. `None` without an owner, as every hook of the file.
unsafe fn with_view<R>(view: *const u8, f: impl FnOnce(&FwView) -> R) -> Option<R> {
    crate::DK_SVC_OWNER.step((), |_, ()| (), |(), out| f(&FwView(view, DkFw::new(out))))
}

/// The page the view shows, if it is a native one, cloned out of the owner's table. The view is read between two short entries of the owner (is a page registered; the
/// lookup of the values read), never inside one. `None` also without an owner.
pub fn selected(view: &impl View) -> Option<NativePage> {
    if !crate::DK_SVC_OWNER.with(|m| m.pages().armed())? {
        return None;
    }
    let index = view.page_index();
    if index >= PAGES {
        return None;
    }
    let ty = view.native_type();
    crate::DK_SVC_OWNER
        .with(|m| m.pages().lookup(index, ty).cloned())
        .flatten()
}

/// The draw hook (`mf_draw_hook`): 0 resumes the stock draw, 1 the page is drawn, 2 asks for the stock draw first and `dks_native_overlay` after it.
#[no_mangle]
pub unsafe extern "C" fn dks_native_draw(view: *const u8, canvas: *mut u8) -> u32 {
    with_view(view, |v| {
        let Some(page) = selected(v) else {
            return Draw::Stock as u32;
        };
        let vptr = word(canvas, 0);
        draw_page(
            &page,
            v,
            &mut FwSurface { vptr },
            &mut FwStock { canvas, vptr },
            &mut *addr_of_mut!(CLIPPED),
            &mut FwContent,
        ) as u32
    })
    .unwrap_or(Draw::Stock as u32)
}

/// The second half of an overlay page, after the stock draw.
#[no_mangle]
pub unsafe extern "C" fn dks_native_overlay(view: *const u8, canvas: *mut u8) {
    with_view(view, |v| {
        let Some(page) = selected(v) else {
            return;
        };
        let vptr = word(canvas, 0);
        overlay_page(
            &page,
            v,
            &mut FwSurface { vptr },
            &mut FwStock { canvas, vptr },
            &mut *addr_of_mut!(CLIPPED),
            &mut FwContent,
        );
    });
}

/// The parameter code of an encoder on the page the view shows: -1 resumes the stock mapper.
pub fn code(view: &impl View, encoder: u32) -> i32 {
    selected(view).map_or(-1, |page| page.encoder_code(encoder))
}

/// The parameter code of an encoder (`mf_code_hook`): -1 resumes the stock mapper.
#[no_mangle]
pub unsafe extern "C" fn dks_native_code(view: *const u8, encoder: u32) -> i32 {
    with_view(view, |v| code(v, encoder)).unwrap_or(-1)
}
/// The number of pages of machine type `ty`, given `ordinary` pages of its own.
#[no_mangle]
pub unsafe extern "C" fn dks_native_page_count(ty: u32, ordinary: u32) -> u32 {
    crate::DK_SVC_OWNER
        .with(|m| m.pages().page_count(ty, ordinary))
        .unwrap_or(ordinary)
}

/// The page descriptor the firmware reads for page `page` of machine type `ty`, or 0.
#[no_mangle]
pub unsafe extern "C" fn dks_native_descriptor(ty: u32, page: u32) -> u32 {
    crate::DK_SVC_OWNER
        .with(|m| {
            m.pages()
                .page(ty, page)
                .map_or(0, |p| p.descriptor_ptr() as usize as u32)
        })
        .unwrap_or(0)
}

/// The turn of an encoder on a native page: nonzero when the page consumed it. Runs outside the owner.
pub unsafe fn turn(view: *const u8, ev: *const u8) -> u32 {
    with_view(view, |v| {
        let Some(page) = selected(v) else {
            return 0;
        };
        let event = Event {
            control: word(ev, EV_CONTROL),
            delta: word(ev, EV_DELTA) as i32,
            coarse: *ev.add(EV_COARSE) != 0,
        };
        turn_page(&page, v, &event, &mut FwContent)
    })
    .unwrap_or(0)
}
