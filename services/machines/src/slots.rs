//! `machines` service, slots and hooks: the machine page's key and turn slots and the encoder click on it (docs/design/machine-page-click.md).
//!
//! A machine module declares `DK_PARAM_ACTION(machine, word, fn)`. The packer lowers it to an ACTION record of the `register` table; `registry::apply`
//! stores the callback's address per parameter row (`set`). Two hooks at the entry of the machine page's key handler and turn handler
//! (`dkrt_pagekey_hook`, `dkrt_pageturn_hook`, src/cf/tramp.s) call `dkrt_page_key` and `dkrt_page_turn` (`handlers.rs` of the runtime crate), which call
//! `page_key` and `page_turn` here with the runtime's core and firmware page.
//! Ordinary PARAM turns and all keys continue through the stock handler. An opt-in native page's LOCAL/unused turn returns one to consume it.
//!
//! A click is a press of an encoder's key (codes 0x29 to 0x30) and its release with no turn in between. Press and turn is the stock coarse step
//! and never a click. The parameter under the encoder comes from the page itself (vtable slot 47 of the view, the call the stock list-opener
//! uses): the dispatch is by parameter row, so it does not depend on the device's knob layout. The callback runs from the UI loop pass
//! that follows the release (`tick`), in the UI task, with the track number: `void fn(u32 track)`.
//!
//! Without any action or native page, the hooks return after their armed checks.
use super::paramapi::{self, Ctx, Held};
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::fw::Fw;
use dkrt_sdk::limits;

mod action;
mod detector;
#[cfg(test)]
mod tests;

pub use action::{find, resolve};
pub use detector::*;

/// The page's track reference (`view + 0x74`, the argument of `cf.page.fn.view_track`).
const VIEW_TRACK_REF: usize = 0x74;

static mut DET: Detector = Detector::new();
static mut ACTIONS: [u32; limits::PARAM_ROWS] = [0; limits::PARAM_ROWS];
/// The `arg` of an action registered through `dk_param_on_click` : such a callback is `void fn(void *arg, u32 track)`.
static mut ARGS: [(bool, u32); limits::PARAM_ROWS] = [(false, 0); limits::PARAM_ROWS];
/// The number of parameter rows that have an action: 0 keeps both hooks at one test.
static mut ARMED: u32 = 0;
/// A click waiting for the next UI pass: the callback (0: none), the track, the clicked view and the clicked parameter code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    pub callback: u32,
    pub track: u32,
    pub view: usize,
    pub code: u32,
}
const NO_CLICK: Pending = Pending {
    callback: 0,
    track: 0,
    view: 0,
    code: 0,
};
static mut PENDING: Pending = NO_CLICK;
/// The click flow that `param_get` and `param_set` may serve: from the callback until the page gets input again.
static mut HELD: Held = Held::new();

/// Set (`addr` != 0) or clear (0) the action of parameter row `k` (a row of core/dynreg). Called by the registry.
pub unsafe fn set(k: usize, addr: u32) {
    if k >= limits::PARAM_ROWS {
        return;
    }
    let a = &mut *addr_of_mut!(ACTIONS);
    let old = a[k];
    a[k] = addr;
    (*addr_of_mut!(ARGS))[k] = (false, 0);
    let mut n = 0;
    let mut i = 0;
    while i < limits::PARAM_ROWS {
        if a[i] != 0 {
            n += 1;
        }
        i += 1;
    }
    write_volatile(addr_of_mut!(ARMED), n);
    // a click of an unloaded module must not run
    let p = &mut *addr_of_mut!(PENDING);
    if addr == 0 && old != 0 {
        if p.callback == old {
            *p = NO_CLICK;
        }
        (*addr_of_mut!(HELD)).input(); // the module that owns the flow is going away
    }
}

/// Give the action of parameter row `k` an argument (the callback is then called `fn(arg, track)`).
pub unsafe fn set_arg(k: usize, arg: u32) {
    if k < limits::PARAM_ROWS {
        (*addr_of_mut!(ARGS))[k] = (true, arg);
    }
}

pub(crate) unsafe fn word(p: *const u8, off: usize) -> u32 {
    read_volatile(p.add(off) as *const u32)
}

/// Hook at the entry of the page's key handler (stack: view, event).
pub unsafe fn page_key<C: CoreApi, F: Fw>(core: &C, fw: &F, view: *const u8, ev: *const u8) {
    if read_volatile(addr_of!(ARMED)) == 0 {
        return;
    }
    (*addr_of_mut!(HELD)).input();
    let det = &mut *addr_of_mut!(DET);
    if let Some(enc) = det.key(view as usize, word(ev, 0xc), word(ev, 0x10)) {
        click(core, fw, view, enc);
    }
}

/// Hook at the entry of the page's turn handler (stack: view, event). Only an encoder event (`event[3] - 1` is 0 to 7) is a turn.
pub unsafe fn page_turn<C: CoreApi>(core: &C, view: *const u8, ev: *const u8) -> u32 {
    // Every page turn ends the click flow, including LOCAL and unused custom
    // encoders. Bookkeeping must precede the consumed return: press A, turn an
    // unused D, release A must never become an SMP click.
    if read_volatile(addr_of!(ARMED)) != 0 {
        turn_input(
            &mut *addr_of_mut!(DET),
            &mut *addr_of_mut!(HELD),
            view as usize,
            word(ev, 0xc),
        );
    }
    // PARAM continues through the native handler exactly once; LOCAL/unused is
    // consumed only after recording the turn above.
    super::nativepage::turn(core, view, ev)
}

pub fn turn_input(detector: &mut Detector, held: &mut Held, view: usize, encoder: u32) {
    held.input();
    if encoder.wrapping_sub(1) < ENCODERS {
        detector.turn(view);
    }
}

/// A click of encoder `enc` on `view`: queue the action of the parameter under it, if it has one.
unsafe fn click<C: CoreApi, F: Fw>(core: &C, fw: &F, view: *const u8, enc: u32) {
    let code = fw.param_code(view as usize, enc);
    let first = crate::records::param_first(core);
    let track = || fw.view_track(word(view, VIEW_TRACK_REF) as usize);
    if let Some((callback, track)) = resolve(&*addr_of!(ACTIONS), first, code, track) {
        *addr_of_mut!(PENDING) = Pending {
            callback,
            track,
            view: view as usize,
            code: code as u32,
        };
    }
}

/// The context of a click: the owner is the module of the clicked parameter row.
pub fn context(p: &Pending, owners: &[u8], first: u32) -> Option<Ctx> {
    let k = p.code.wrapping_sub(first) as usize;
    match owners.get(k) {
        Some(&owner) if owner != 0 => Some(Ctx {
            view: p.view,
            track: p.track,
            owner,
        }),
        _ => None,
    }
}

/// The context of the queued click: the owner is read from the core's parameter rows.
pub fn click_context<C: CoreApi>(c: &C, p: &Pending) -> Option<Ctx> {
    context(
        p,
        crate::records::param_owners(),
        crate::records::param_first(c),
    )
}

/// The UI loop pass: run the queued action (`void fn(u32 track)`). `param_get` and `param_set` serve the clicked view from here until the page
/// gets input again, so the list the action opens can set the value when the user picks.
pub unsafe fn tick<C: CoreApi>(core: &C) {
    let p = *addr_of!(PENDING);
    if p.callback != 0 {
        *addr_of_mut!(PENDING) = NO_CLICK;
        let Some(ctx) = click_context(core, &p) else {
            return;
        };
        (*addr_of_mut!(HELD)).begin(ctx);
        let (with_arg, arg) =
            (*addr_of!(ARGS))[(p.code.wrapping_sub(crate::records::param_first(core))) as usize
                % limits::PARAM_ROWS];
        if with_arg {
            let call: unsafe extern "C" fn(u32, u32) = core::mem::transmute(p.callback as usize);
            call(arg, p.track);
        } else {
            let call: unsafe extern "C" fn(u32) = core::mem::transmute(p.callback as usize);
            call(p.track);
        }
    }
}

/// `dk_param_get`.
pub unsafe fn param_get<C: CoreApi, F: Fw>(core: &C, fw: &F, track: u32, word: u32) -> i32 {
    paramapi::get(
        fw,
        (*addr_of!(HELD)).ctx(),
        crate::records::param_owners(),
        crate::records::param_first(core),
        track,
        word,
    )
}

/// `dk_param_set`.
pub unsafe fn param_set<C: CoreApi, F: Fw>(
    core: &C,
    fw: &F,
    track: u32,
    word: u32,
    raw: i32,
) -> i32 {
    paramapi::set(
        fw,
        (*addr_of!(HELD)).ctx(),
        crate::records::param_owners(),
        crate::records::param_first(core),
        track,
        word,
        raw,
    )
}

/// The click that waits for the next UI pass (for the tests of the registry: what `apply` armed). `#[inline]`: no caller in the runtime, so no code in the object.
#[inline]
pub unsafe fn pending() -> Pending {
    *addr_of!(PENDING)
}
