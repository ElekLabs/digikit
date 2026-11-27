//! `BoundProject` over a reload: write its fields into the carried bytes, take them back, or start unbound.
use super::*;
use crate::engine::carry::{CarryIn, CarryOut};
use crate::name::{Path, PATH_MAX};

impl BoundProject {
    /// The most bytes `export` writes.
    pub(in crate::engine) const EXPORT_MAX: usize = dkp::AREA + 160;

    pub(in crate::engine) fn export(&self, now: u32, out: &mut CarryOut) {
        let (tag, slot, retry_at) = match self.binding {
            Binding::None => (0, 0, 0),
            Binding::Bound(slot) => (1, slot, 0),
            Binding::LoadFailed { slot, retry_at } => (2, slot, retry_at),
        };
        out.u8(tag);
        out.u32(slot);
        out.deadline(now, if tag == 2 { retry_at } else { now });
        out.opt_u32(self.live_stamp);
        out.u8(self.file.is_some() as u8);
        let file = self.file.unwrap_or(SourceFile {
            head: dkp::Head {
                device: 0,
                slot: 0,
                name_len: 0,
                name: [0; 16],
                stamp: 0,
                counter: 0,
                used: 0,
            },
            path: Path::EMPTY,
        });
        let head = &file.head;
        out.u16(head.device);
        out.u8(head.slot);
        out.u8(head.name_len);
        out.bytes(&head.name);
        out.u32(head.stamp);
        out.u32(head.counter);
        out.u32(head.used);
        out.u8(file.path.n as u8);
        out.bytes(&file.path.b);
        out.u8(self.dirty as u8);
        out.u32(self.generation);
        out.u8(self.ignored as u8);
        out.u32(self.frozen_generation);
        out.u8(matches!(self.start, StartBind::Named) as u8);
        out.u32(self.epoch);
        out.bytes(&self.live.0);
    }

    /// Take the fields written by `export`; `None` when the bytes are not a state of this layout (a field may be half taken then: the caller starts unbound).
    /// Every field is checked with the rules of a file (`dkp::head_ok`, `dkp::image_ok`, the store path and the device); `now` rebases the carried deadline.
    pub(in crate::engine) fn import(
        &mut self,
        now: u32,
        device: u16,
        from: &mut CarryIn,
    ) -> Option<()> {
        let tag = from.u8()?;
        let slot = from.u32()?;
        let retry_at = from.deadline(now)?;
        if slot > 127 && tag != 0 {
            return None;
        }
        self.binding = match tag {
            0 => Binding::None,
            1 => Binding::Bound(slot),
            2 => Binding::LoadFailed { slot, retry_at },
            _ => return None,
        };
        self.live_stamp = from.opt_u32()?;
        let has_file = from.flag()?;
        let head = dkp::Head {
            device: from.u16()?,
            slot: from.u8()?,
            name_len: from.u8()?,
            name: from.bytes(16)?.try_into().ok()?,
            stamp: from.u32()?,
            counter: from.u32()?,
            used: from.u32()?,
        };
        let n = from.u8()? as usize;
        let b: [u8; PATH_MAX] = from.bytes(PATH_MAX)?.try_into().ok()?;
        self.file = if has_file {
            if head.device != device || !dkp::head_ok(&head) {
                return None;
            }
            Some(SourceFile {
                head,
                path: Path::from_store_bytes(b.get(..n)?)?,
            })
        } else {
            None
        };
        self.dirty = from.flag()?;
        self.generation = from.u32()?;
        self.ignored = from.flag()?;
        self.frozen_generation = from.u32()?;
        self.start = if from.flag()? {
            StartBind::Named
        } else {
            StartBind::Unnamed
        };
        self.epoch = from.u32()?;
        self.live.0.copy_from_slice(from.bytes(dkp::AREA)?);
        let empty = self.live.0.iter().all(|&b| b == 0);
        (empty || dkp::image_ok(&self.live.0)).then_some(())
    }

    /// No project is current and the start is named: module writes go to the settings record until the setter names a project. The image is empty.
    pub(in crate::engine) fn start_unbound(&mut self) {
        self.live.0.fill(0);
        self.binding = Binding::None;
        self.live_stamp = None;
        self.file = None;
        self.dirty = false;
        self.generation = 0;
        self.ignored = false;
        self.frozen_generation = 0;
        self.start = StartBind::Named;
        self.epoch = 0;
    }
}
