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
use dkrt_sdk::limits;

mod action;
mod detector;
#[cfg(test)]
mod tests;

pub use action::{find, resolve};
pub use detector::*;

/// The page's track reference (`view + 0x74`, the argument of `cf.page.fn.view_track`).
const VIEW_TRACK_REF: usize = 0x74;

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

/// The state of the page's clicks, in the owner: the detector, the action and argument of each parameter row, the click that waits for the next pass and the flow that
/// `param_get` and `param_set` may serve.
pub struct Slots {
    det: Detector,
    actions: [u32; limits::PARAM_ROWS],
    /// The `arg` of an action registered through `dk_param_on_click` : such a callback is `void fn(void *arg, u32 track)`.
    args: [(bool, u32); limits::PARAM_ROWS],
    /// The number of parameter rows that have an action: 0 keeps both hooks at one test.
    armed: u32,
    pending: Pending,
    /// The click flow that `param_get` and `param_set` may serve: from the callback until the page gets input again.
    held: Held,
}

impl Default for Slots {
    fn default() -> Slots {
        Slots::new()
    }
}

impl Slots {
    pub const fn new() -> Slots {
        Slots {
            det: Detector::new(),
            actions: [0; limits::PARAM_ROWS],
            args: [(false, 0); limits::PARAM_ROWS],
            armed: 0,
            pending: NO_CLICK,
            held: Held::new(),
        }
    }

    /// Set (`addr` != 0) or clear (0) the action of parameter row `k` (a row of core/rows). Called by `apply` and `remove`.
    pub fn set(&mut self, k: usize, addr: u32) {
        if k >= limits::PARAM_ROWS {
            return;
        }
        let old = self.actions[k];
        self.actions[k] = addr;
        self.args[k] = (false, 0);
        let mut n = 0;
        let mut i = 0;
        while i < limits::PARAM_ROWS {
            if self.actions[i] != 0 {
                n += 1;
            }
            i += 1;
        }
        // SAFETY: a field of the owner; the volatile write keeps the hooks' reads honest
        unsafe { write_volatile(addr_of_mut!(self.armed), n) };
        // a click of an unloaded module must not run
        if addr == 0 && old != 0 {
            if self.pending.callback == old {
                self.pending = NO_CLICK;
            }
            self.held.input(); // the module that owns the flow is going away
        }
    }

    /// Give the action of parameter row `k` an argument (the callback is then called `fn(arg, track)`).
    pub fn set_arg(&mut self, k: usize, arg: u32) {
        if k < limits::PARAM_ROWS {
            self.args[k] = (true, arg);
        }
    }

    /// Whether any parameter row has an action.
    fn is_armed(&self) -> bool {
        // SAFETY: as in `set`
        unsafe { read_volatile(addr_of!(self.armed)) != 0 }
    }

    /// First half of the key hook: the detector. `Some(encoder)` is a click. The code of the encoder is asked of the page by the caller, outside the owner: the page's code hook
    /// (`dks_native_code`) enters the owner to look up a native page, and inside this call it would be refused and the click would map to the stock code.
    pub unsafe fn key_click(&mut self, view: *const u8, ev: *const u8) -> Option<u32> {
        if !self.is_armed() {
            return None;
        }
        self.held.input();
        self.det.key(view as usize, word(ev, 0xc), word(ev, 0x10))
    }

    /// The page's track reference of `view`: the argument of the firmware's `view_track`, which the caller calls outside the owner (it is a firmware call).
    pub unsafe fn track_ref(view: *const u8) -> usize {
        word(view, VIEW_TRACK_REF) as usize
    }

    /// Second half: queue the action of parameter `code`, the code `view` maps the clicked encoder to, if it has one. `track` is the page's track (`view_track` of
    /// [`Slots::track_ref`], asked of the firmware outside the owner).
    pub unsafe fn queue_click<C: CoreApi>(
        &mut self,
        core: &C,
        view: *const u8,
        code: i32,
        track: u32,
    ) {
        let first = crate::records::param_first(core);
        if let Some((callback, track)) = resolve(&self.actions, first, code, || track) {
            self.pending = Pending {
                callback,
                track,
                view: view as usize,
                code: code as u32,
            };
        }
    }

    /// Hook at the entry of the page's turn handler (stack: view, event): every page turn ends the click flow, including LOCAL and unused custom encoders. Bookkeeping must
    /// precede the consumed return: press A, turn an unused D, release A must never become an SMP click. The native handler (`nativepage::turn`) runs after it, outside
    /// the owner.
    pub unsafe fn page_turn_input(&mut self, view: *const u8, ev: *const u8) {
        if self.is_armed() {
            turn_input(&mut self.det, &mut self.held, view as usize, word(ev, 0xc));
        }
    }

    /// Open the flow of a click (what `take_click` does for a queued one).
    pub(crate) fn begin_flow(&mut self, ctx: Ctx) {
        self.held.begin(ctx);
    }

    /// The queued click, taken for the UI pass: its callback, and the argument it was registered with. The flow of `param_get` and `param_set` is open from here until the
    /// page gets input again, so the list the action opens can set the value when the user picks. `None`: nothing queued, or the row was unloaded.
    unsafe fn take_click<C: CoreApi>(
        &mut self,
        core: &C,
        owners: &[u8],
    ) -> Option<(Pending, (bool, u32))> {
        let p = self.pending;
        if p.callback == 0 {
            return None;
        }
        self.pending = NO_CLICK;
        let ctx = click_context(core, owners, &p)?;
        self.held.begin(ctx);
        let arg = self.args[(p.code.wrapping_sub(crate::records::param_first(core))) as usize
            % limits::PARAM_ROWS];
        Some((p, arg))
    }

    /// The UI loop pass: run the queued action (`void fn(u32 track)`). The callback is the last thing that runs: it may call `dk_param_get` and `dk_param_set`, which reach
    /// the flow through the owner again.
    pub unsafe fn tick<C: CoreApi>(&mut self, core: &C, owners: &[u8]) {
        let Some((p, (with_arg, arg))) = self.take_click(core, owners) else {
            return;
        };
        if with_arg {
            let call: unsafe extern "C" fn(u32, u32) = core::mem::transmute(p.callback as usize);
            call(arg, p.track);
        } else {
            let call: unsafe extern "C" fn(u32) = core::mem::transmute(p.callback as usize);
            call(p.track);
        }
    }

    /// `dk_param_get` and `dk_param_set`, owner half: what the call resolves to (`paramapi::target`); the firmware half (`paramapi::read`, `write`) runs outside the owner.
    pub unsafe fn param_target<C: CoreApi>(
        &self,
        core: &C,
        owners: &[u8],
        track: u32,
        word: u32,
    ) -> Result<paramapi::Target, i32> {
        paramapi::target(
            self.held.ctx(),
            owners,
            crate::records::param_first(core),
            track,
            word,
        )
    }

    /// The click that waits for the next UI pass (for the tests of the registry: what `apply` armed).
    pub fn pending(&self) -> Pending {
        self.pending
    }
}

pub(crate) unsafe fn word(p: *const u8, off: usize) -> u32 {
    read_volatile(p.add(off) as *const u32)
}

pub fn turn_input(detector: &mut Detector, held: &mut Held, view: usize, encoder: u32) {
    held.input();
    if encoder.wrapping_sub(1) < ENCODERS {
        detector.turn(view);
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
pub fn click_context<C: CoreApi>(c: &C, owners: &[u8], p: &Pending) -> Option<Ctx> {
    context(p, owners, crate::records::param_first(c))
}
