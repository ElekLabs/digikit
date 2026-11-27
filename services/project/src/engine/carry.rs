//! `ProjectService` over a reload of the service file (the `State` part of the owner): what the old copy hands the new one and the rule for when it may.
//!
//! The service may be taken down when nothing is in flight: its stage is idle, no store job is posted and the hooks' work is started (`can_carry`). Then the state is the bound
//! project (live image, binding, file, dirty flag, generation, epoch, start flag), the restore of the module list, the deferred flag and the hooks' record. The hooks' record is
//! outside the engine (the file's static, written by the hooks until they are restored), so the owner passes it in. Unsaved edits stay in RAM: nothing is written to the store.
//! A listing, a task run or an unstarted queue is not carried: the stage is idle and the queue empty by the rule above, and the clock's hold and busy time are fresh.
//!
//! Layout of the bytes (all integers big-endian): the bound project, the restore, one byte for the deferred flag, the hooks' record. `VERSION` changes when it does.
mod reader;
mod writer;

pub(super) use reader::CarryIn;
pub(super) use writer::CarryOut;

use super::{BoundProject, Env, ModuleRestore, Pending, ProjectService, Stage, StoreJob};

/// The layout of the carried bytes.
pub const VERSION: u16 = 1;

/// The most bytes `export` writes.
pub const EXPORT_MAX: usize = BoundProject::EXPORT_MAX + ModuleRestore::EXPORT_MAX + 1 + 4 * 4 + 32;

impl ProjectService {
    /// Is the service in a state that a copy can take over: no stage runs, no job is posted and no work waits in the queue?
    pub fn can_carry(&self, job: &StoreJob) -> bool {
        self.stage == Stage::Idle && !job.is_posted() && self.queue.is_empty()
    }

    /// Write the state and the hooks' record `hooks` into `out` (at least `EXPORT_MAX`): the count written.
    pub fn export_state(&self, hooks: &Pending, out: &mut [u8]) -> usize {
        let mut w = CarryOut::new(out);
        self.bound.export(&mut w);
        self.restore.export(&mut w);
        w.u8(self.clock.is_deferred() as u8);
        w.u32(hooks.slot_val);
        w.u32(hooks.slot_flag);
        w.u32(hooks.save_slot);
        w.u32(hooks.save_flag);
        w.bytes(&hooks.clear);
        w.bytes(&hooks.paste);
        w.finish()
    }

    /// Take the bytes of `export_state` of a copy of the same layout and put its hooks' record into `hooks`. `None`, or bytes that do not parse, give the start of a project that is
    /// named and unbound. The fields change in place.
    pub fn import_state<E: Env + ?Sized>(&mut self, env: &E, hooks: &Pending, from: Option<&[u8]>) {
        let taken = from.and_then(|bytes| self.take(env, hooks, bytes));
        if taken.is_none() {
            self.bound.start_unbound();
            self.restore.start_empty();
            self.clock.restore_deferred(false);
        }
    }

    fn take<E: Env + ?Sized>(&mut self, env: &E, hooks: &Pending, bytes: &[u8]) -> Option<()> {
        let mut r = CarryIn::new(bytes);
        self.bound.import(&mut r)?;
        self.restore.import(&mut r)?;
        let deferred = r.flag()?;
        let record = Pending {
            slot_val: r.u32()?,
            slot_flag: r.u32()?,
            save_slot: r.u32()?,
            save_flag: r.u32()?,
            clear: r.bytes(16)?.try_into().ok()?,
            paste: r.bytes(16)?.try_into().ok()?,
        };
        if !r.is_done() {
            return None;
        }
        self.clock.restore_deferred(deferred);
        hooks.put_back(env, &record);
        Some(())
    }
}
