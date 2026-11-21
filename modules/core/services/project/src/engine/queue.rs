//! The work the hooks asked for and the tick has not started yet. UI task only.
use super::pending::HookRecords;
use super::{BoundProject, Env, ScanFor, SlotScan};
use crate::dkp::Ignore;
use dkrt_sdk::logk;

/// A save whose slot the firmware's list does not know yet waits this long (the list is refreshed after the save).
pub const REC_WAIT_MS: u32 = 2000;

/// One bit per slot 0 to 127 (byte `slot / 8`, bit `slot % 8`), the layout of the hooks' bitmaps.
#[derive(Clone, Copy)]
pub struct SlotSet([u8; 16]);

impl SlotSet {
    pub const fn new() -> SlotSet {
        SlotSet([0; 16])
    }
    fn merge(&mut self, bits: &[u8; 16]) {
        let mut at = 0;
        while at < 16 {
            self.0[at] |= bits[at];
            at += 1;
        }
    }
    fn insert(&mut self, slot: u32) {
        self.0[(slot / 8) as usize] |= 1 << (slot % 8);
    }
    /// Remove and return the lowest slot of the set.
    fn take_first(&mut self) -> Option<u32> {
        let mut at = 0;
        while at < 16 {
            if self.0[at] != 0 {
                let mut bit = 0;
                while self.0[at] & (1 << bit) == 0 {
                    bit += 1;
                }
                self.0[at] &= !(1 << bit);
                return Some(at as u32 * 8 + bit);
            }
            at += 1;
        }
        None
    }
}

/// A piece of work to start, with the slot it is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Work {
    /// The slot was cleared: its files go to the archive.
    Clear(u32),
    /// The slot was pasted over: its old files go to the archive and the source's file is copied to it.
    Paste(u32),
    /// A stock save of the slot ended: write the image.
    Save(u32),
    /// The slot became current: read its file.
    Load(u32),
    /// The setter got a value that is no slot (-1 or the live mirror): the project is left and none is current.
    Leave { setter_value: u32 },
}

impl Work {
    /// The scan this work starts with, and its slot; `None` for work that needs no listing. (If-let chain, not a `match`: see `Task::prepare`.)
    pub fn scan(&self) -> Option<(ScanFor, u32)> {
        if let Work::Clear(slot) = *self {
            Some((ScanFor::Clear, slot))
        } else if let Work::Paste(slot) = *self {
            Some((ScanFor::Paste, slot))
        } else if let Work::Save(slot) = *self {
            Some((ScanFor::Save, slot))
        } else if let Work::Load(slot) = *self {
            Some((ScanFor::Load, slot))
        } else {
            None
        }
    }
}

/// What the hooks asked for and the tick has not started: the slot to load, the slot saved, the slots cleared and pasted over, and the clock of a save that waits for the firmware's record of its slot.
/// `ProjectService::tick` (UI task) fills it from the hooks and takes the next [`Work`] when the stage is idle.
pub struct WorkQueue {
    load: Option<u32>,
    save: Option<u32>,
    cleared: SlotSet,
    pasted: SlotSet,
    /// A save that waits for the firmware's record of its slot: since when.
    save_wait: Option<u32>,
}

impl WorkQueue {
    pub const fn new() -> WorkQueue {
        WorkQueue {
            load: None,
            save: None,
            cleared: SlotSet::new(),
            pasted: SlotSet::new(),
            save_wait: None,
        }
    }

    /// Queue what the hooks recorded. A later setter call or save replaces an earlier one that was not started.
    pub fn add(&mut self, records: &HookRecords) {
        if let Some(slot) = records.slot_set {
            self.load = Some(slot);
        }
        if let Some(slot) = records.saved_slot {
            self.save = Some(slot);
        }
        self.cleared.merge(&records.cleared);
        self.pasted.merge(&records.pasted);
    }

    /// Put work back whose scan was refused: it is the next to start again, unless newer work of its kind came in meanwhile.
    pub fn requeue(&mut self, scan: &SlotScan) {
        let (scan_for, slot) = (scan.scan_for(), scan.slot());
        if scan_for == ScanFor::Clear {
            self.cleared.insert(slot);
        } else if scan_for == ScanFor::Paste {
            self.pasted.insert(slot);
        } else if scan_for == ScanFor::Save {
            self.save.get_or_insert(slot);
        } else {
            self.load.get_or_insert(slot);
        }
    }

    /// Queue a load of `slot` (no hook asked for it).
    pub fn add_load(&mut self, slot: u32) {
        self.load = Some(slot);
    }

    /// Queue the quiet retry of a failed load of `slot`, unless a load is queued already.
    pub fn add_retry(&mut self, slot: u32) {
        self.load.get_or_insert(slot);
    }

    /// The next piece of work, or `None` when there is none yet: first the files of cleared and pasted slots, then a save, then a load.
    pub fn next<E: Env + ?Sized>(&mut self, bound: &BoundProject, env: &mut E) -> Option<Work> {
        if let Some(slot) = self.cleared.take_first() {
            return Some(Work::Clear(slot));
        }
        if let Some(slot) = self.pasted.take_first() {
            return Some(Work::Paste(slot));
        }
        // the stock setter writes only on a change: a call with the slot that is current is no load
        if self.load.map(|slot| slot as i32 == bound.slot().as_i32()) == Some(true) {
            self.load = None;
        }
        // a save and a load of the same slot in one pass: a save-as keeps the data, a New project does not (`image_matches_live`)
        if let (Some(load), Some(save)) = (self.load, self.save) {
            if load == save && bound.is_bound() && bound.image_matches_live(&*env) {
                self.load = None;
            }
        }
        // a save into the slot whose load failed has no image to write and must not archive the slot's file: it is skipped (one IGNORED event) and the load goes again
        if let (Some(save), Some(failed)) = (self.save, bound.failed_load()) {
            if save == failed {
                env.log(
                    logk::PROJECT_IGNORED,
                    [save, Ignore::SaveSkipped as u32, 0, 0, 0],
                );
                self.save = None;
                self.load.get_or_insert(save);
            }
        }
        if let Some(save) = self.save {
            if save > 127 {
                self.save = None;
            } else if self.load.is_none() {
                return self.next_save(env, save);
            }
            // else: a load first, then the save
        }
        if let Some(load) = self.load {
            if env.live_op() != 0 {
                return None;
            }
            self.load = None;
            if load > 127 {
                return Some(Work::Leave { setter_value: load });
            }
            return Some(Work::Load(load));
        }
        None
    }

    /// The save of `slot`: the firmware's list is refreshed after the save, so wait for the record of the slot a little; after `REC_WAIT_MS` the save is dropped.
    fn next_save<E: Env + ?Sized>(&mut self, env: &mut E, slot: u32) -> Option<Work> {
        if env.slot_record(slot).is_none() {
            let now = env.now_ms();
            let since = *self.save_wait.get_or_insert(now);
            if now.wrapping_sub(since) < REC_WAIT_MS {
                return None;
            }
            self.save_wait = None;
            self.save = None;
            env.log(
                logk::PROJECT_IGNORED,
                [slot, Ignore::SaveNoRecord as u32, 0, 0, 0],
            );
            return None;
        }
        self.save_wait = None;
        self.save = None;
        Some(Work::Save(slot))
    }
}
