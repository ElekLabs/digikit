//! Freezing the image for a write and marking it written.
use crate::dkp::{self, Image};
use crate::engine::bound::*;
use crate::engine::{Env, SlotRecord};
use crate::name::{self, Path};

impl BoundProject {
    /// Freeze the image for a write: its header gets the slot, name and stamp as the firmware lists them now and the write counter `counter`, then its bytes are copied to
    /// `buffer`. Returns their number. A put after this leaves the image dirty (`mark_written`).
    pub fn freeze_into<E: Env + ?Sized>(
        &mut self,
        env: &E,
        buffer: &mut Image,
        slot: u8,
        record: &SlotRecord,
        counter: u32,
    ) -> u32 {
        let mut head = dkp::head_of(&self.live.0);
        head.device = env.device();
        head.slot = slot;
        head.name = record.name;
        head.name_len = name::trimmed_len_raw(&record.name) as u8;
        head.stamp = record.stamp;
        dkp::set_head(&mut self.live.0, &head);
        dkp::seal(&mut self.live.0, counter);
        let used = dkp::head_of(&self.live.0).used as usize;
        buffer.0[..used].copy_from_slice(&self.live.0[..used]);
        self.frozen_generation = self.generation;
        used as u32
    }

    /// The bytes that `freeze_into` froze were written as `to`: the image belongs to that file now, and is clean unless a put came after the freeze.
    pub fn mark_written(&mut self, to: Path, slot: u8, record: &SlotRecord, counter: u32) {
        let mut head = dkp::head_of(&self.live.0);
        head.slot = slot;
        head.stamp = record.stamp;
        head.name = record.name;
        head.counter = counter;
        self.file = Some(SourceFile { head, path: to });
        if self.generation == self.frozen_generation {
            self.dirty = false;
        }
    }
}
