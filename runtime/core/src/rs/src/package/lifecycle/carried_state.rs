//! The state a reload of a service carries to its next copy (`CarriedState`): one arena block for each service of the closure that has the `State` part, taken before the
//! take-down so that a full arena refuses the reload while nothing has changed, filled when the old copy has stopped taking calls (`export`), read by the new copy right after
//! its `start` (`import`), and freed on every end of the reload (`release_all`). The bytes live in RAM only; no persisted format changes.
//!
//! The runtime checks what the service's own code cannot: the name hash and the version range. Bytes that do not pass are dropped and the new copy gets `import(None)`, the same
//! call a fresh start gets; the service decides what that means for it. Everything here runs over two small seams (`Blocks`, `Donor`), so the host tests give fakes.
use crate::package::loader::Block;
use dkrt_abi::dev_addr::DevAddr;

use dkrt_abi::MAX_MODULES;
use dkrt_sdk::{E_NOMEM, OK, PROVIDER_MAX};

/// The most bytes one service may carry (what `export_max` may ask for).
pub(crate) const CAP: usize = 64 * 1024;

/// Where the blocks come from: the loader's arena on the device.
pub(crate) trait Blocks {
    unsafe fn alloc(&mut self, n: u32) -> Option<Block>;
    unsafe fn free(&mut self, addr: DevAddr);
}

/// One copy of a service, as the state handoff sees it: its name, the versions of its `State` part (`None`: it has none) and the three calls of the part.
pub(crate) trait Donor {
    fn name(&self) -> u32;
    fn versions(&self) -> Option<(u16, u16)>;
    unsafe fn export_max(&self) -> usize;
    unsafe fn export(&self, out: &mut [u8]) -> usize;
    /// The old copy's `(version, bytes)`, or none; the service's status.
    unsafe fn import(&self, from: Option<(u16, &[u8])>) -> i32;
}

/// Why bytes were not handed over (the `outcome` word of the log event).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Dropped {
    /// The new copy is another service (a different name hash).
    Name = 1,
    /// The old layout is outside what the new copy accepts.
    Version = 2,
    /// The new copy has no `State` part.
    NoPart = 3,
}

/// What `import` did, for the log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Outcome {
    /// Nothing was carried: the new copy started fresh (it got `import(None)` when it has the part).
    Fresh,
    /// The bytes of `len` and layout `version` went to the new copy.
    Carried { version: u16, len: u32 },
    /// The bytes of `len` and layout `version` were dropped.
    Dropped { why: Dropped, version: u16, len: u32 },
}

/// One service's place in the carry: its phases are the states of the slot.
#[derive(Clone, Copy)]
enum Slot {
    Empty,
    /// The block is taken, the old copy has not written into it.
    Reserved { block: Block, cap: u32 },
    /// The old copy wrote `len` bytes of layout `version` and name `name`.
    Exported { block: Block, cap: u32, len: u32, version: u16, name: u32 },
}

pub(crate) struct CarriedState {
    slots: [Slot; PROVIDER_MAX],
}

fn slot_of(rec: usize) -> Option<usize> {
    rec.checked_sub(MAX_MODULES).filter(|j| *j < PROVIDER_MAX)
}

impl CarriedState {
    pub(crate) const NEW: CarriedState = CarriedState { slots: [Slot::Empty; PROVIDER_MAX] };

    /// Take the blocks of the service records among `members`, one for each service that has the part and asks for room: `donor_of(rec)` is its running copy. All or
    /// nothing: `E_NOMEM` (a service asks for more than `CAP`, or the arena has no block) frees what was taken, and nothing has changed.
    pub(crate) unsafe fn reserve<D: Donor>(&mut self, members: &[usize], donor_of: impl Fn(usize) -> Option<D>, arena: &mut impl Blocks) -> i32 {
        for &rec in members {
            let (Some(j), Some(donor)) = (slot_of(rec), donor_of(rec)) else {
                continue;
            };
            if donor.versions().is_none() {
                continue;
            }
            let want = donor.export_max();
            if want == 0 {
                continue;
            }
            let block = if want > CAP { None } else { arena.alloc(want as u32) };
            let Some(block) = block else {
                self.release_all(arena);
                return E_NOMEM;
            };
            self.slots[j] = Slot::Reserved { block, cap: want as u32 };
        }
        OK
    }

    /// The arena bytes the taken blocks hold (as the arena rounds them): the part of the arena the reload cannot count as free.
    pub(crate) fn held_bytes(&self) -> u32 {
        let mut n = 0u32;
        for s in &self.slots {
            if let Slot::Reserved { cap, .. } | Slot::Exported { cap, .. } = s {
                n += dkneeds::arena_block(*cap).unwrap_or(*cap);
            }
        }
        n
    }

    /// The old copy of service record `rec` stopped taking calls: it writes its state into the block (nothing for a service that took none).
    pub(crate) unsafe fn export<D: Donor>(&mut self, rec: usize, donor: &D) {
        let Some(j) = slot_of(rec) else {
            return;
        };
        if let Slot::Reserved { block, cap } = self.slots[j] {
            let out = core::slice::from_raw_parts_mut(block.ptr, cap as usize);
            let len = donor.export(out).min(cap as usize) as u32;
            let version = donor.versions().map_or(0, |v| v.0);
            self.slots[j] = Slot::Exported { block, cap, len, version, name: donor.name() };
        }
    }

    /// The new copy of service record `rec` has started: it gets the old copy's bytes when the name and the version range allow, else `import(None)`. The service's status
    /// and what happened.
    pub(crate) unsafe fn import<D: Donor>(&mut self, rec: usize, donor: &D) -> (i32, Outcome) {
        let held = match slot_of(rec).map(|j| self.slots[j]) {
            Some(Slot::Exported { block, len, version, name, .. }) if len > 0 => Some((block, len, version, name)),
            _ => None,
        };
        let Some((block, len, version, name)) = held else {
            return (if donor.versions().is_some() { donor.import(None) } else { OK }, Outcome::Fresh);
        };
        let Some((newest, from)) = donor.versions().map(|(v, f)| (v, f)) else {
            return (OK, Outcome::Dropped { why: Dropped::NoPart, version, len });
        };
        let why = if donor.name() != name {
            Some(Dropped::Name)
        } else if version < from || version > newest {
            Some(Dropped::Version)
        } else {
            None
        };
        match why {
            Some(why) => (donor.import(None), Outcome::Dropped { why, version, len }),
            None => {
                let bytes = core::slice::from_raw_parts(block.ptr as *const u8, len as usize);
                (donor.import(Some((version, bytes))), Outcome::Carried { version, len })
            }
        }
    }

    /// Free every block (an end of the reload, whatever its result).
    pub(crate) unsafe fn release_all(&mut self, arena: &mut impl Blocks) {
        for s in self.slots.iter_mut() {
            match *s {
                Slot::Reserved { block, .. } | Slot::Exported { block, .. } => arena.free(block.addr),
                Slot::Empty => {}
            }
            *s = Slot::Empty;
        }
    }

    /// Does any block wait?
    pub(crate) fn is_held(&self) -> bool {
        self.slots.iter().any(|s| !matches!(s, Slot::Empty))
    }
}

#[cfg(test)]
mod tests;
