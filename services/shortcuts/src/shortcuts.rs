//! `Shortcuts`: the owner object of the service and its state machine. One key event goes in (`on_key`), the verdict says whether the stock firmware may see it.
//!
//! A binding is complete when its first key is held and its second key is pressed. The second key and its release (and its repeats) are swallowed: the stock views
//! never see them, so a combination like [TRK]+[YES] does not also act as YES. The first key is never swallowed (the stock firmware needs [TRK] itself). `on` runs when
//! the binding completes and `off` when either key is released; every other event passes, so a trig key pressed under [TRK] still selects a track.
use crate::bindings::{Binding, BindingTable, Callback};
use crate::builder::Builder;
use crate::keys::KeyState;
use core::ffi::c_void;
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::platform::DeviceProfile;
use dkrt_abi::provider::{ConsumerId, Provider, Reentry, Status, Ticker};
use dkrt_abi::service::tick_phase;

/// The bits of the `flags` of a key event.
pub mod key_flags {
    /// The key went up (clear: it went down).
    pub const RELEASE: u32 = 1;
    /// The press repeats (the key is held and the firmware repeats it).
    pub const REPEAT: u32 = 2;
}

/// The `key_flags` of a firmware key event: its flags word (`KeyEvent+0x10`) read with the device's bits (`[input.key_flags]`). A release is classified first: a clear
/// press bit is a release whatever the other bits say; the repeat bit on a press is a repeat.
pub fn classify_flags(profile: &DeviceProfile, fw: u32) -> u32 {
    let bits = profile.key_flags;
    if fw >> bits.press & 1 == 0 {
        key_flags::RELEASE
    } else if fw >> bits.repeat & 1 != 0 {
        key_flags::REPEAT
    } else {
        0
    }
}

/// What the key-dispatch hook does with the event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// The stock firmware sees the event.
    Pass,
    /// A shortcut took the event: the stock firmware does not see it.
    Swallow,
}

/// Calls the hook has decided on and `run_pending` has not made yet.
pub const PENDING_MAX: usize = 16;

/// One call of a module's `on` or `off`, waiting for the UI pass.
#[derive(Clone, Copy)]
struct Pending {
    owner: u8,
    call: Option<Callback>,
    arg: *mut c_void,
}

/// The owner object of the service: no key held, no binding, nothing active, nothing pending. The file's cell holds the one instance (`Provider::start`), and everything
/// that reaches it (the part functions, the key hook, the builder call) runs in the UI task, so the fields need no lock.
pub struct Shortcuts {
    /// The calls to make, oldest first: `head` is the oldest and `len` the count (a ring).
    pending: [Pending; PENDING_MAX],
    head: usize,
    len: usize,
    held: KeyState,
    table: BindingTable,
    /// The binding that is active (its `on` ran, no key of it has been released since): its index in `table` plus 1; 0 when none (so all-zero bytes are the initial state).
    active: usize,
    /// The builder call `dk_shortcut_add` and its collection (`Records::begin`, `commit`).
    builder: Builder,
}

/// No binding is active.
const NONE: usize = 0;

impl Shortcuts {
    pub const fn new() -> Shortcuts {
        Shortcuts {
            pending: [Pending {
                owner: 0,
                call: None,
                arg: core::ptr::null_mut(),
            }; PENDING_MAX],
            head: 0,
            len: 0,
            held: KeyState::new(),
            table: BindingTable::new(),
            active: NONE,
            builder: Builder::new(),
        }
    }

    /// Remember a call of `owner`'s module code for `run_pending`. A full queue drops the call (a module that does not get its UI pass for 16 key events has other trouble).
    fn defer(&mut self, owner: u8, call: Option<Callback>, arg: *mut c_void) {
        if call.is_some() && self.len < PENDING_MAX {
            let at = (self.head + self.len) % PENDING_MAX;
            self.pending[at] = Pending { owner, call, arg };
            self.len += 1;
        }
    }

    /// Make the calls the key events decided on, in the order of the events (`on`, then `off` of a fast tap). The UI pass calls this (`Ticker::tick`), never the key hook:
    /// the hook runs inside the firmware's key walker and must not run module code. Each call is taken out of the queue before it runs.
    ///
    /// # Safety
    /// The callbacks are module code that is loaded (`detach` drops the calls of a module that unloads).
    unsafe fn run_pending(&mut self) {
        while self.len > 0 {
            let p = self.pending[self.head];
            self.head = (self.head + 1) % PENDING_MAX;
            self.len -= 1;
            if let Some(call) = p.call {
                call(p.arg);
            }
        }
    }

    /// Calls waiting for `run_pending`.
    pub fn pending(&self) -> usize {
        self.len
    }

    pub fn table(&self) -> &BindingTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut BindingTable {
        &mut self.table
    }

    /// Drop the bindings of `owner` (its module unloads) and the calls of it that wait for `run_pending`. An active binding of the owner ends first, without calling `off`: its code is going away. The key state stays as it is,
    /// because the keys are still physically down: the first key stays held (a binding added later completes with it), and the press of the second key stays swallowed, so its release is
    /// swallowed once (the stock firmware never saw the press).
    fn drop_owner(&mut self, owner: u8) {
        if let Some(b) = self.active() {
            if b.owner == owner {
                self.active = NONE;
            }
        }
        self.table.remove(owner);
        let mut kept = 0;
        let mut i = 0;
        let mut q = self.pending;
        while i < self.len {
            let p = self.pending[(self.head + i) % PENDING_MAX];
            if p.owner != owner {
                q[kept] = p;
                kept += 1;
            }
            i += 1;
        }
        self.pending = q;
        self.head = 0;
        self.len = kept;
    }

    /// The builder of `dk_shortcut_add` and the records' commit (`Builder::add`, `begin`, `commit`).
    pub fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    /// One key event of the panel for the dispatch hook: `code` is the key's id in the event, `flags` the event's flags word (read with `[input.key_flags]`) of the device
    /// `core` runs as; a device the table does not know passes everything. It decides and queues the module's calls; it runs no module code (the UI pass does, `Ticker::tick`).
    pub fn dispatch<C: CoreApi>(&mut self, core: &C, code: u32, flags: u32) -> Verdict {
        match DeviceProfile::by_id(core.device_id()) {
            Some(p) => self.on_key(&p, code, classify_flags(&p, flags)),
            None => Verdict::Pass,
        }
    }

    /// One key event of the panel: `code` is the device's panel code, `flags` are [`key_flags`]. A code that no logical key of the device has passes.
    /// It only decides: the `on` and `off` of a binding are queued (`run_pending` makes them in the UI pass), so no module code runs inside the firmware's key walker.
    pub fn on_key(&mut self, profile: &DeviceProfile, code: u32, flags: u32) -> Verdict {
        let key = profile.logical_key(code);
        if key == 0 {
            return Verdict::Pass;
        }
        if flags & key_flags::RELEASE != 0 {
            self.held.release(key);
            let verdict = if self.held.take_swallowed(key) {
                Verdict::Swallow
            } else {
                Verdict::Pass
            };
            if let Some(b) = self.active().copied() {
                if u32::from(b.keys[0]) == key || u32::from(b.keys[1]) == key {
                    self.active = NONE;
                    self.defer(b.owner, b.off, b.arg);
                }
            }
            return verdict;
        }
        if flags & key_flags::REPEAT != 0 {
            return if self.held.is_swallowed(key) {
                Verdict::Swallow
            } else {
                Verdict::Pass
            };
        }
        // a press of a key whose press was swallowed and not released yet is a repeat that the firmware encodes as a plain press
        if self.held.is_swallowed(key) {
            return Verdict::Swallow;
        }
        self.held.press(key);
        if self.active == NONE {
            let held = self.held;
            if let Some(i) = self.table.completed(key, |k| held.is_held(k)) {
                if let Some(b) = self.table.get(i).copied() {
                    self.active = i + 1;
                    self.held.swallow(key);
                    self.defer(b.owner, b.on, b.arg);
                    return Verdict::Swallow;
                }
            }
        }
        Verdict::Pass
    }

    /// The binding that is active now (for a hint or a test).
    pub fn active(&self) -> Option<&Binding> {
        self.active.checked_sub(1).and_then(|i| self.table.get(i))
    }
}

impl<C: CoreApi> Provider<C> for Shortcuts {
    fn start(_core: &C) -> Result<Shortcuts, Status> {
        Ok(Shortcuts::new())
    }

    /// The state is made where it lives: all-zero bytes are the initial state (see `Shortcuts::new`; `tests/state.rs` checks it), so this is one fill and no copy of
    /// the 1.4 kB through the stack.
    fn start_in(_core: &C, slot: &mut MaybeUninit<Shortcuts>) -> Result<(), Status> {
        // SAFETY: every field of `Shortcuts` is valid as zero bytes (integers, `None` callbacks, null pointers, the closed gate).
        unsafe { slot.as_mut_ptr().write_bytes(0, 1) };
        Ok(())
    }

    /// A module unloads (also one that never attached, or only in part): close its collection and drop its bindings and queued calls (`drop_owner`). It never calls module
    /// code, `off` of an active binding included.
    fn detach(&mut self, _core: &C, consumer: ConsumerId) {
        self.builder.end();
        self.drop_owner(consumer.get());
    }
}

impl<C: CoreApi> Ticker<C> for Shortcuts {
    /// The UI pass: make the `on` and `off` calls that the key hook queued, in order, in the early phase only.
    fn tick(&mut self, _core: &C, phase: u32) {
        if phase == tick_phase::EARLY {
            // SAFETY: the queued callbacks are code of modules that are loaded: `detach` drops the calls of a module before its image goes.
            unsafe { self.run_pending() };
        }
    }
}

/// One key event of the panel, as the key hook saw it while the service was inside a call: the hook cannot decide it then (the object is in use), so the owner queues it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    pub code: u32,
    pub flags: u32,
}

impl<C: CoreApi> Reentry<C> for Shortcuts {
    type Event = KeyEvent;
    /// A call of the service is short (one tick runs the queued `on` and `off` calls): a few keys at most arrive meanwhile.
    const QUEUE: usize = 8;

    /// Take the key after the call it arrived in: its state counts (a held first key, a release), so no key is lost. The stock walker has seen the event already, so the verdict
    /// has no one to go to.
    fn apply(&mut self, core: &C, event: KeyEvent) {
        let _ = self.dispatch(core, event.code, event.flags);
    }
}
