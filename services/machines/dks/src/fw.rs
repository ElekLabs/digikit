//! The firmware calls of the machines service file: `DkFw`, the firmware's parameter page as the service reaches it (`dkrt_abi::fw::Fw`; the page's vtable slots as
//! `RuntimeFw` did, the same offsets, and the three firmware functions through the `dkfw` crate, whose names `patchkit pack` turns into `dk_fw_` imports that the port file
//! resolves), and the entries that use it: the page's key and turn (called by the hooks), and the exports `dk_param_get` and `dk_param_set`. A host build has no firmware, and a firmware function that is not bound (address 0, `guard.rs`):
//! the calls are no-ops with the neutral answer.
use core::ptr::read_volatile;
use dkrt_abi::fw::{Fw, MODE_KEYS};
use dkrt_svc_machines::paramapi;
use dkrt_svc_machines::slots::{Click, Slots};
use dkrt_svc_rt::{ImportCore, Outside};

/// The firmware's parameter page. Made from the [`Outside`] token that `Owner::step` gives its `act` phase and nothing else gives, so no code inside the owner can hold one: a
/// firmware call that calls back into this service (a code hook, a setter that fires hooks) always finds the owner idle.
pub struct DkFw(());

impl DkFw {
    pub fn new(_: &Outside) -> DkFw {
        DkFw(())
    }

    /// The handle for the page's getters inside a hook that has no step (`native.rs`: the page the view shows is looked up with the owner held). Only the getters of the stock
    /// page (`view_track`, `page_value`, the type readers) may be called through it: they fire no hook of this service. Anything that stores or runs a callback takes a token.
    pub(crate) fn pure_reads() -> DkFw {
        DkFw(())
    }
}

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
    page_key(DkFw::new, view, ev);
}

/// The key hook over any `Fw` made from the token (`DkFw::new` for the firmware; the host test of the file gives a fake that enters the owner, as the page's code hook does).
/// Decide inside the owner, ask the firmware outside it (the encoder's code and the page's track), act inside again.
pub unsafe fn page_key<F: Fw>(make: impl FnOnce(&Outside) -> F, view: *const u8, ev: *const u8) {
    let asked = crate::DK_SVC_OWNER.step(
        (view, ev),
        |m, (view, ev)| m.clicks().key_click(view, ev),
        |enc, out| {
            let fw = make(out);
            enc.map(|enc| {
                (
                    fw.param_code(view as usize, enc),
                    fw.view_track(Slots::track_ref(view)),
                )
            })
        },
    );
    if let Some(Some((code, track))) = asked {
        crate::DK_SVC_OWNER.with(|m| m.clicks().queue_click(&ImportCore, view, code, track));
    }
}

/// The tick of the file (the UI task, once per pass; `tick =` of `service!`): the click that waits runs outside the owner, so its callback may call `dk_param_get` and
/// `dk_param_set`, which enter it (SHOT's open-picker reads the current slot). The owner only takes the click; before `init` and after `fini` there is none.
pub unsafe extern "C" fn tick(_ctx: *mut u8) {
    click_tick(|click| click.run());
}

/// The tick over any way to run the taken click (the host test of the file runs a closure for a module's callback, whose address would not fit a host pointer).
pub unsafe fn click_tick(run: impl FnOnce(Click)) {
    crate::DK_SVC_OWNER.step(
        (),
        |m, ()| m.take_click(&ImportCore),
        |click, _| {
            if let Some(click) = click {
                run(click);
            }
        },
    );
}

/// The hook entry of the machine page's turn: the turn ends the click flow (the owner), then the native handler of the page runs outside the owner: nonzero when a native page
/// consumed the turn. Without an owner nothing is consumed.
#[no_mangle]
pub unsafe extern "C" fn dks_page_turn(view: *const u8, ev: *const u8) -> u32 {
    match crate::DK_SVC_OWNER.with(|m| m.clicks().page_turn_input(view, ev)) {
        Some(()) => crate::native::turn(view, ev),
        None => 0,
    }
}

/// The copy at the end of the firmware's machine list getter (C ABI `copy(begin, end, dest)`, result d0 = the end of the copy): the `jsr` of a device with a slot map is pointed here by a patch
/// (`list_copy_target`), the hook `mf_copy` of a device with page rows calls it. It leaves out the reserved machine rows that have no name; it reads the registry words bound by `start` and
/// needs no owner.
#[no_mangle]
pub unsafe extern "C" fn dks_list_copy(begin: u32, end: u32, dest: u32) -> u32 {
    dkrt_svc_machines::list::copy_listed(
        &ImportCore,
        &dkrt_svc_machines::list::Reserved::bound(),
        begin,
        end,
        dest,
    )
}

/// The value of the patch that points the list copy of a device with a slot map here (`service!`).
pub fn list_copy_target(_: &dkrt_svc_machines::machines::Machines) -> u32 {
    dks_list_copy as usize as u32
}

/// `dk_param_get` (an `[[export]]` of the file): the value of a parameter word in the flow of a click. Without an owner there is no flow (`E_STATE`).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_get(track: u32, word: u32) -> i32 {
    param_get(DkFw::new, track, word)
}

/// `dk_param_set`, as `dk_param_get`.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_set(track: u32, word: u32, raw: i32) -> i32 {
    param_set(DkFw::new, track, word, raw)
}

/// `dk_param_get` over any `Fw` made from the token: the owner resolves the target, the page is read outside it.
pub unsafe fn param_get<F: Fw>(make: impl FnOnce(&Outside) -> F, track: u32, word: u32) -> i32 {
    crate::DK_SVC_OWNER
        .step(
            (track, word),
            |m, (track, word)| m.param_target(&ImportCore, track, word),
            |t, out| match t {
                Ok(t) => paramapi::read(&make(out), &t),
                Err(e) => e,
            },
        )
        .unwrap_or(paramapi::E_STATE)
}

/// `dk_param_set` over any `Fw` made from the token, as `param_get`: the stock setter fires hooks, which find the owner idle.
pub unsafe fn param_set<F: Fw>(
    make: impl FnOnce(&Outside) -> F,
    track: u32,
    word: u32,
    raw: i32,
) -> i32 {
    crate::DK_SVC_OWNER
        .step(
            (track, word),
            |m, (track, word)| m.param_target(&ImportCore, track, word),
            |t, out| match t {
                Ok(t) => paramapi::write(&make(out), &t, raw),
                Err(e) => e,
            },
        )
        .unwrap_or(paramapi::E_STATE)
}
