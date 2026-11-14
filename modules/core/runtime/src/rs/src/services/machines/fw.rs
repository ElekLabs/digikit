//! The firmware calls of the `machines` service, behind `paramapi::Fw`: the page's vtable slots, the key state, the stock parameter setter and the
//! view's track. This is the service's own adapter interface (not the core's `Platform`); the file is the one place of the service that names `dkfw`
//! (`check:layers`) and moves with the machines adapter at R3b.

use super::paramapi;
use super::pageclick::word;

/// The page's own functions, as a knob turn uses them (docs/design/ui-rules.md, "Runtime API for parameter-driven flows").
#[cfg(not(test))]
pub struct PageFw;

/// Vtable slots of the page (`view`) and of its parameter object.
#[cfg(not(test))]
const VT_PAGE_VALUE: usize = 0x54;
#[cfg(not(test))]
const VT_PAGE_OBJECT: usize = 0xc4;
#[cfg(not(test))]
const VT_OBJECT_VALUE: usize = 0x28;

#[cfg(not(test))]
unsafe fn vcall(obj: usize, off: usize) -> usize {
    word(word(obj as *const u8, 0) as *const u8, off) as usize
}

#[cfg(not(test))]
impl paramapi::Fw for PageFw {
    fn mode_key_held(&self) -> bool {
        paramapi::MODE_KEYS.iter().any(|&k| unsafe { dkfw::input::key_down(k) } != 0)
    }
    fn page_value(&self, view: usize, code: u32) -> i32 {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32, i32) -> i32 = core::mem::transmute(vcall(view, VT_PAGE_VALUE));
            f(view, code, -1)
        }
    }
    fn object(&self, view: usize, code: u32, track: u32) -> usize {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32, u32) -> usize = core::mem::transmute(vcall(view, VT_PAGE_OBJECT));
            f(view, code, track)
        }
    }
    fn object_value(&self, obj: usize, code: u32) -> i32 {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32) -> i32 = core::mem::transmute(vcall(obj, VT_OBJECT_VALUE));
            f(obj, code)
        }
    }
    fn store(&self, obj: usize, code: u32, raw: i32, track: u32) {
        unsafe {
            dkfw::param::set_value(obj as *const u8, code, raw, track, 1, 1, 1);
        }
    }
    fn view_track(&self, track_ref: usize) -> u32 {
        unsafe { dkfw::page::view_track(track_ref as *const u8) }
    }
}

