//! The firmware calls of the machines service file: `DkFw`, the firmware's parameter page as the service reaches it (`dkrt_abi::fw::Fw`; the page's vtable slots as
//! `RuntimeFw` did, the same offsets, and the three firmware functions through the `dkfw` crate, whose names `patchkit pack` turns into `dk_fw_` imports that the port file
//! resolves), and the entries that use it: the page's key and turn (called by the hooks), and the exports `dk_param_get` and `dk_param_set`. A host build has no firmware, and a firmware function that is not bound (address 0, `guard.rs`):
//! the calls are no-ops with the neutral answer.
use core::ptr::read_volatile;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::fw::{Fw, MODE_KEYS};
use dkrt_sdk::OK;
use dkrt_svc_machines::slots;
use dkrt_svc_rt::ImportCore;

pub struct DkFw;

/// The three firmware functions of the click flow (`cf.input.fn.key_down`, `cf.param.fn.set_value`, `cf.page.fn.view_track`).
#[cfg(target_arch = "m68k")]
mod native {
    use crate::guard::call_if_bound;

    pub unsafe fn key_down(key: u32) -> u32 {
        call_if_bound(dkfw::input::key_down as usize, 0, || dkfw::input::key_down(key))
    }
    pub unsafe fn set_value(obj: *const u8, code: u32, raw: i32, track: u32) {
        call_if_bound(dkfw::param::set_value as usize, (), || {
            dkfw::param::set_value(obj, code, raw, track, 1, 1, 1);
        })
    }
    pub unsafe fn view_track(track_ref: *const u8) -> u32 {
        call_if_bound(dkfw::page::view_track as usize, 0, || dkfw::page::view_track(track_ref))
    }
    pub unsafe fn filter_row(index: u32) -> u32 {
        call_if_bound(dkfw::filter::accessor as usize, 0, || dkfw::filter::accessor(index) as usize as u32)
    }
    pub unsafe fn alloc(size: u32) -> u32 {
        call_if_bound(dkfw::mem::new as usize, 0, || dkfw::mem::new(size) as usize as u32)
    }
    pub unsafe fn free(p: u32) {
        call_if_bound(dkfw::mem::free as usize, (), || dkfw::mem::free(p as usize as *mut u8))
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
    pub unsafe fn filter_row(_: u32) -> u32 {
        0
    }
    pub unsafe fn alloc(_: u32) -> u32 {
        0
    }
    pub unsafe fn free(_: u32) {}
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

fn tab() -> DkFw {
    DkFw
}

/// The hook entry of the machine page's key event: the click detector of the service.
#[no_mangle]
pub unsafe extern "C" fn dks_page_key(view: *const u8, ev: *const u8) {
    slots::page_key(&ImportCore, &tab(), view, ev)
}

/// The hook entry of the machine page's turn: nonzero when a native page consumed it.
#[no_mangle]
pub unsafe extern "C" fn dks_page_turn(view: *const u8, ev: *const u8) -> u32 {
    slots::page_turn(&ImportCore, view, ev)
}

/// Bind the machine image table, then read the stock Multi-Mode descriptor row that module filters copy.
pub unsafe fn setup<C: CoreApi>(core: &C) -> i32 {
    let status = crate::image::setup(core);
    if status == OK {
        dkrt_svc_machines::filter::set_stock(native::filter_row(0));
    }
    status
}

/// The routine of the list hook: the order vector the stock function filled gets the loaded module filters appended (the hook calls it with the vector object).
#[no_mangle]
pub unsafe extern "C" fn dks_filter_list(vec: u32) {
    dkrt_svc_machines::filter::extend_list(&ImportCore, vec, &mut |n| native::alloc(n), &mut |p| native::free(p));
}

/// `dk_param_get` (an `[[export]]` of the file): the value of a parameter word in the flow of a click.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_get(track: u32, word: u32) -> i32 {
    slots::param_get(&ImportCore, &tab(), track, word)
}

/// `dk_param_set`, as `dk_param_get`.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_set(track: u32, word: u32, raw: i32) -> i32 {
    slots::param_set(&ImportCore, &tab(), track, word, raw)
}

/// Draw the loaded filter's response into the stock FLTR bitmap before the shared page tail.
#[no_mangle]
pub unsafe extern "C" fn dks_filter_graph(view: *mut u8, bitmap: *mut u8, ty: u32) {
    use crate::image::{w, MF_TAB};
    use dkrt_svc_machines::filter;

    if !(filter::FIRST..=filter::LAST).contains(&ty) {
        return;
    }
    let index = (ty - filter::FIRST) as usize;
    let row = &filter::rows()[index];
    let adapter = MF_TAB[w::N_FILTER_GRAPH];
    if row.owner == 0 || row.draw == 0 || adapter == 0 {
        return;
    }
    let draw: unsafe extern "C" fn(*mut u8, *mut u8, u32, *const u32, u32) =
        core::mem::transmute(adapter as usize);
    let codes = &filter::tab().rows[index][filter::KNOB_WORD..];
    draw(view, bitmap, row.draw, codes.as_ptr(), 1 + u32::from(row.knobs));
}
