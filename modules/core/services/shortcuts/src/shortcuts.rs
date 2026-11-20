//! `Shortcuts`: the state machine of the service. One key event goes in (`on_key`), the verdict says whether the stock firmware may see it.
//!
//! A binding is complete when its first key is held and its second key is pressed. The second key and its release (and its repeats) are swallowed: the stock views
//! never see them, so a combination like [TRK]+[YES] does not also act as YES. The first key is never swallowed (the stock firmware needs [TRK] itself). `on` runs when
//! the binding completes and `off` when either key is released; every other event passes, so a trig key pressed under [TRK] still selects a track.
use crate::bindings::{Binding, BindingTable};
use crate::keys::KeyState;
use dkrt_abi::platform::DeviceProfile;

/// The bits of the `flags` of a key event.
pub mod key_flags {
    /// The key went up (clear: it went down).
    pub const RELEASE: u32 = 1;
    /// The press repeats (the key is held and the firmware repeats it).
    pub const REPEAT: u32 = 2;
}

/// What the key-dispatch hook does with the event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// The stock firmware sees the event.
    Pass,
    /// A shortcut took the event: the stock firmware does not see it.
    Swallow,
}

/// All-zero bytes are the initial state (no key held, no binding, nothing active).
pub struct Shortcuts {
    held: KeyState,
    table: BindingTable,
    /// The binding that is active (its `on` ran, no key of it has been released since): its index in `table` plus 1; 0 when none (so all-zero bytes are the initial state).
    active: usize,
    /// The logical keys whose press was swallowed and whose release is still to come, as a bit set: their release and repeats are swallowed too.
    swallowed: u64,
}

/// No binding is active.
const NONE: usize = 0;

impl Shortcuts {
    pub const fn new() -> Shortcuts {
        Shortcuts {
            held: KeyState::new(),
            table: BindingTable::new(),
            active: NONE,
            swallowed: 0,
        }
    }

    pub fn table(&self) -> &BindingTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut BindingTable {
        &mut self.table
    }

    /// Drop the bindings of `owner` (its module unloads). An active binding of the owner ends first, without calling `off`: its code is going away.
    pub fn remove(&mut self, owner: u8) {
        if let Some(b) = self.active() {
            if b.owner == owner {
                self.active = NONE;
            }
        }
        self.table.remove(owner);
    }

    fn mark(key: u32) -> u64 {
        if key == 0 || key >= 64 {
            0
        } else {
            1u64 << key
        }
    }

    /// One key event of the panel: `code` is the device's panel code, `flags` are [`key_flags`]. A code that no logical key of the device has passes.
    ///
    /// # Safety
    /// The `on` and `off` callbacks of the bindings are called: they are module code that is loaded (`remove` drops them with the module).
    pub unsafe fn on_key(&mut self, profile: &DeviceProfile, code: u32, flags: u32) -> Verdict {
        let key = profile.logical_key(code);
        if key == 0 {
            return Verdict::Pass;
        }
        let mark = Shortcuts::mark(key);
        if flags & key_flags::RELEASE != 0 {
            self.held.release(key);
            let verdict = if self.swallowed & mark != 0 {
                Verdict::Swallow
            } else {
                Verdict::Pass
            };
            self.swallowed &= !mark;
            if let Some(b) = self.active().copied() {
                if u32::from(b.keys[0]) == key || u32::from(b.keys[1]) == key {
                    self.active = NONE;
                    if let Some(off) = b.off {
                        off(b.arg);
                    }
                }
            }
            return verdict;
        }
        if flags & key_flags::REPEAT != 0 {
            return if self.swallowed & mark != 0 {
                Verdict::Swallow
            } else {
                Verdict::Pass
            };
        }
        self.held.press(key);
        if self.active == NONE {
            let held = self.held;
            if let Some(i) = self.table.completed(key, |k| held.is_held(k)) {
                if let Some(b) = self.table.get(i).copied() {
                    self.active = i + 1;
                    self.swallowed |= mark;
                    if let Some(on) = b.on {
                        on(b.arg);
                    }
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
