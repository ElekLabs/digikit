//! `dk_project_list`.
use super::entry::{ProjectEntry, ENTRY_CURRENT, ENTRY_DATA, ENTRY_UNKNOWN};
use crate::engine::{Env, ProjectService};
use crate::name;

/// The slots of the firmware's list: 0 to 127.
const SLOTS: u32 = 128;

impl ProjectService {
    /// The projects of the firmware's list from slot `first` upward, up to `out.len()` entries: the number written. Slot, name and current come from the firmware's records (no I/O);
    /// the data bit comes from the data index.
    pub fn list<E: Env + ?Sized>(&self, env: &E, first: u32, out: &mut [ProjectEntry]) -> usize {
        let current = self.bound.slot().index();
        let mut count = 0;
        let mut slot = first;
        while slot < SLOTS && count < out.len() {
            if let Some(record) = env.slot_record(slot) {
                let mut flags = 0;
                if current == Some(slot) {
                    flags |= ENTRY_CURRENT;
                }
                if !self.index.known() {
                    flags |= ENTRY_UNKNOWN;
                } else if self.index.has(slot) {
                    flags |= ENTRY_DATA;
                }
                let entry = &mut out[count];
                entry.slot = slot as u8;
                entry.flags = flags;
                entry.name_len = name::trimmed_len_raw(&record.name) as u8;
                entry.name = record.name;
                count += 1;
            }
            slot += 1;
        }
        count
    }
}
