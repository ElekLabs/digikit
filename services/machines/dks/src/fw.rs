//! The firmware calls of the machines service file: `DkFw`, the firmware's parameter page as the service reaches it (`dkrt_abi::fw::Fw`; the page's vtable slots as
//! `RuntimeFw` did, the same offsets, and the three firmware functions through the `dkfw` crate, whose names `patchkit pack` turns into `dk_fw_` imports that the port file
//! resolves), and the entries that use it: the page's key and turn (called by the hooks), and the exports `dk_param_get` and `dk_param_set`. A host build has no firmware, and a firmware function that is not bound (address 0, `guard.rs`):
//! the calls are no-ops with the neutral answer.
use core::ptr::read_volatile;
use dkrt_abi::fw::{Fw, MODE_KEYS};
use dkrt_svc_machines::{nativepage, paramapi};
use dkrt_svc_rt::ImportCore;

pub struct DkFw;

/// The three firmware functions of the click flow (`cf.input.fn.key_down`, `cf.param.fn.set_value`, `cf.page.fn.view_track`).
#[cfg(target_arch = "m68k")]
mod native {
    use crate::guard::call_if_bound;

    pub unsafe fn key_down(key: u32) -> u32 {
        call_if_bound(dkfw::input::key_down as usize, 0, || {
            dkfw::input::key_down(key)
        })
    }
    pub unsafe fn set_value(obj: *const u8, code: u32, raw: i32, track: u32) {
        call_if_bound(dkfw::param::set_value as usize, (), || {
            dkfw::param::set_value(obj, code, raw, track, 1, 1, 1);
        })
    }
    pub unsafe fn view_track(track_ref: *const u8) -> u32 {
        call_if_bound(dkfw::page::view_track as usize, 0, || {
            dkfw::page::view_track(track_ref)
        })
    }
}
#[cfg(not(target_arch = "m68k"))]
mod native {
    pub unsafe fn key_down(_: u32) -> u32 {
        0
    }
    pub unsafe fn set_value(_: *const u8, _: u32, _: i32, _: u32) {}
    pub unsafe fn view_track(_: *const u8) -> u32 {
        0
    }
}

/// Vtable slots of the page (`view`) and of its parameter object.
const VT_PAGE_VALUE: usize = 0x54;
const VT_PAGE_OBJECT: usize = 0xc4;
const VT_OBJECT_VALUE: usize = 0x28;
/// The page's `encoder -> parameter code` function: vtable slot 47 (offset 0xbc), `i32 (view, encoder)`.
const VT_PARAM_CODE: usize = 4 * 47;

unsafe fn word(p: usize, off: usize) -> usize {
    read_volatile((p + off) as *const u32) as usize
}

unsafe fn vcall(obj: usize, off: usize) -> usize {
    word(word(obj, 0), off)
}

impl Fw for DkFw {
    fn mode_key_held(&self) -> bool {
        MODE_KEYS
            .iter()
            .any(|&k| unsafe { native::key_down(k) } != 0)
    }
    fn page_value(&self, view: usize, code: u32) -> i32 {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32, i32) -> i32 =
                core::mem::transmute(vcall(view, VT_PAGE_VALUE));
            f(view, code, -1)
        }
    }
    fn object(&self, view: usize, code: u32, track: u32) -> usize {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32, u32) -> usize =
                core::mem::transmute(vcall(view, VT_PAGE_OBJECT));
            f(view, code, track)
        }
    }
    fn object_value(&self, obj: usize, code: u32) -> i32 {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32) -> i32 =
                core::mem::transmute(vcall(obj, VT_OBJECT_VALUE));
            f(obj, code)
        }
    }
    fn store(&self, obj: usize, code: u32, raw: i32, track: u32) {
        unsafe { native::set_value(obj as *const u8, code, raw, track) }
    }
    fn view_track(&self, track_ref: usize) -> u32 {
        unsafe { native::view_track(track_ref as *const u8) }
    }
    fn param_code(&self, view: usize, enc: u32) -> i32 {
        unsafe {
            let f: unsafe extern "C" fn(usize, u32) -> i32 =
                core::mem::transmute(vcall(view, VT_PARAM_CODE));
            f(view, enc)
        }
    }
}

/// The hook entry of the machine page's key event: the click detector of the owner. Without an owner (before `init` made it, after `fini`) the event passes.
#[no_mangle]
pub unsafe extern "C" fn dks_page_key(view: *const u8, ev: *const u8) {
    crate::DK_SVC_OWNER.with(|m| m.clicks().page_key(&ImportCore, &DkFw, view, ev));
}

/// The hook entry of the machine page's turn: the turn ends the click flow (the owner), then the native handler of the page runs outside the owner: nonzero when a native page
/// consumed the turn. Without an owner nothing is consumed.
#[no_mangle]
pub unsafe extern "C" fn dks_page_turn(view: *const u8, ev: *const u8) -> u32 {
    match crate::DK_SVC_OWNER.with(|m| m.clicks().page_turn_input(view, ev)) {
        Some(()) => nativepage::turn(&ImportCore, view, ev),
        None => 0,
    }
}

/// `dk_param_get` (an `[[export]]` of the file): the value of a parameter word in the flow of a click. Without an owner there is no flow (`E_STATE`).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_get(track: u32, word: u32) -> i32 {
    crate::DK_SVC_OWNER
        .with(|m| m.clicks().param_get(&ImportCore, &DkFw, track, word))
        .unwrap_or(paramapi::E_STATE)
}

/// `dk_param_set`, as `dk_param_get`.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_set(track: u32, word: u32, raw: i32) -> i32 {
    crate::DK_SVC_OWNER
        .with(|m| m.clicks().param_set(&ImportCore, &DkFw, track, word, raw))
        .unwrap_or(paramapi::E_STATE)
}
