//! `ProjectService` over a reload of the service file (the `State` part of the owner): what the old copy hands the new one and the rule for when it may.
//!
//! The service may be taken down when nothing is in flight: its stage is idle, no store job is posted and the hooks' work is started (`can_carry`). Then the state is the bound
//! project (live image, binding, file, dirty flag, generation, epoch, start flag), the restore of the module list, the deferred flag and the hooks' record. The hooks' record is
//! outside the engine (the file's static, written by the hooks until they are restored), so the owner passes it in. Unsaved edits stay in RAM: nothing is written to the store.
//! A listing, a task run or an unstarted queue is not carried: the stage is idle and the queue empty by the rule above, and the clock's hold and busy time are fresh.
//!
//! Deadlines (the retry time, the restore wait) are carried as the ms left and rebased on the new copy's clock, which restarts.
//!
//! Layout of the bytes (all integers big-endian): the bound project, the restore, one byte for the deferred flag, the hooks' record. `VERSION` changes when it does.
mod reader;
mod writer;

pub(super) use reader::CarryIn;
pub(super) use writer::CarryOut;

use super::{BoundProject, Env, ModuleRestore, Pending, ProjectService, Stage, StoreJob};
use crate::dkp::Ignore;
use dkrt_sdk::logk;

/// The layout of the carried bytes.
pub const VERSION: u16 = 2;

/// The longest time left that a carried deadline may hold (the longest wait of the service is `LOAD_RETRY_MS`).
const DEADLINE_MAX_MS: u32 = 60_000;

/// The most bytes `export` writes.
pub const EXPORT_MAX: usize = BoundProject::EXPORT_MAX + ModuleRestore::EXPORT_MAX + 1 + 4 * 4 + 32;

impl ProjectService {
    /// Is the service in a state that a copy can take over: no stage runs, no job is posted and no work waits in the queue?
    pub fn can_carry(&self, job: &StoreJob) -> bool {
        self.stage == Stage::Idle && !job.is_posted() && self.queue.is_empty()
    }

    /// Write the state and the hooks' record `hooks` into `out` (at least `EXPORT_MAX`): the count written.
    pub fn export_state(&self, now: u32, hooks: &Pending, out: &mut [u8]) -> usize {
        let mut w = CarryOut::new(out);
        self.bound.export(now, &mut w);
        self.restore.export(now, &mut w);
        w.u8(self.clock.is_deferred() as u8);
        w.u32(hooks.slot_val);
        w.u32(hooks.slot_flag);
        w.u32(hooks.save_slot);
        w.u32(hooks.save_flag);
        w.bytes(&hooks.clear);
        w.bytes(&hooks.paste);
        w.finish()
    }

    /// Take the bytes of `export_state` of a copy of the same layout and put its hooks' record into `hooks`. `None` (a first start) gives the start of a project that is named and
    /// unbound; bytes that do not parse, or hold a value a file's checks refuse, give the same start with one `PROJECT_IGNORED` log line (slot 0x80, `Ignore::CarryDropped`) and one
    /// toast. The fields change in place.
    pub fn import_state<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        hooks: &Pending,
        from: Option<&[u8]>,
    ) {
        let taken = from.and_then(|bytes| self.take(env, hooks, bytes));
        if taken.is_none() {
            self.bound.start_unbound();
            self.restore.start_empty();
            self.clock.restore_deferred(false);
            // Carried state that was dropped is not silent: `None` is a first start (nothing was carried).
            if from.is_some() {
                let reason = Ignore::CarryDropped as u32;
                env.log(logk::PROJECT_IGNORED, [0x80, reason, 0, 0, 0]);
                env.toast(reason);
            }
        }
    }

    fn take<E: Env + ?Sized>(&mut self, env: &E, hooks: &Pending, bytes: &[u8]) -> Option<()> {
        let mut r = CarryIn::new(bytes);
        let now = env.now_ms();
        self.bound.import(now, env.device(), &mut r)?;
        self.restore.import(now, &mut r)?;
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
