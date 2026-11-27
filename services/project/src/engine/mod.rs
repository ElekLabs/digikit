//! The state and the work of the `project` service (docs/design/digikit-project.md sections 2 to 6): what the tick does after a hook, and the write job.
//!
//! The five hooks of the stock project paths (`hooks.s` of the service file) only record into [`HookRecord`]. [`ProjectService::tick`] runs once per UI pass: it takes what the hooks
//! recorded, finishes a job that ended and advances the stage. Each part owns its state and does its own work:
//!
//! - [`BoundProject`] (`bound.rs`): the current project, its live image, its file; it binds, unbinds, freezes and marks itself written.
//! - [`WorkQueue`] (`queue.rs`): the work the hooks asked for, in the order the tick starts it.
//! - [`SlotScan`] (`scan.rs`): the listing of the store for the files of one slot; it ends in a [`ScanResult`].
//! - `plan.rs`: pure planners from a scan result and a [`BoundView`] to tasks and an [`Effect`].
//! - [`TaskRun`] (`task.rs`): the file operations of one piece of work and how far the run is; a [`Task`] prepares its own job.
//! - [`StoreJob`] (`job.rs`): the cell of the posted I/O job, outside the service: its phase word hands the job's data (the [`StoreWriter`] steps, the [`FileBuffer`]) between the
//!   UI task and the I/O task. The tick gets it as an argument.
//! - [`BusyClock`] (`clock.rs`): the retry, deferral and deferred state of a busy store.
//! - [`ModuleRestore`] (`restore.rs`): after a project's data is installed, the live load of the modules of its module list that are on the card and not loaded.
//!
//! Everything that reaches the outside goes through [`Env`] (the store listing and reads, the firmware's project list, the log, the clock) and [`Files`] (the store writes), so the
//! host tests run it over fakes.
//!
//! Slots: 0 to 127 are projects; the live mirror 0x80 and -1 mean "no current project". The file of a slot is `projects/<number>-<name>.dkp`, the number being slot + 1 ([`crate::name`]).
mod bound;
mod buffer;
mod carry;
mod clock;
mod env;
mod job;
mod pending;
mod plan;
mod queue;
mod restore;
mod scan;
mod task;
mod writer;

pub use bound::{
    Applied, BoundProject, BoundView, Effect, Slot, SourceFile, LIVE_MIRROR, LOAD_RETRY_MS,
};
pub use buffer::FileBuffer;
pub use carry::{EXPORT_MAX as CARRY_MAX, VERSION as CARRY_VERSION};
pub use clock::{is_busy, BusyClock, BusyVerdict, DEFER_MS, RETRY_MS};
pub use env::{stable_record, Env, Files, ListPoll, ListRow, SlotRecord};
pub use job::StoreJob;
pub use pending::{HookRecord, HookRecords, Pending};
pub use plan::Plan;
pub use queue::{Work, WorkQueue, REC_WAIT_MS};
pub use restore::{
    Event, LiveJob, LoadedModule, ModuleId, ModulePath, ModuleRestore, ModuleSet, ModuleWarning,
    CARD_TRIES_MAX, CARD_WAIT_MS, ID_MAX, MODULES_MAX, MODULE_PATH_MAX, PLAY_WAIT_MS,
};
pub use scan::{ScanFor, ScanResult, ScanStep, SlotFile, SlotScan, SLOT_FILES_MAX};
pub use task::{Prepared, SaveEnd, Task, TaskRun, TASK_MAX};
pub use writer::{JobPoll, JobSpec, StoreWriter, BUSY_RERUNS, IO_MORE};

use crate::dkp::{Ignore, Image};
use dkrt_sdk::logk;

/// `dk_project_status` bits.
pub const ST_FILE: u32 = 1;
pub const ST_DIRTY: u32 = 2;
pub const ST_DEFERRED: u32 = 4;
pub const ST_IGNORED: u32 = 8;

/// What the tick is doing. The data of each stage lives in the owner of that stage ([`ProjectService::scan`], [`ProjectService::run`]).
#[repr(u8)] // an explicit tag, so that Idle is all zero (a niche tag would not be)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Nothing runs; the next tick asks the queue.
    Idle,
    /// A listing runs (the scan).
    Listing,
    /// The tasks of a piece of work run one after the other (the task run).
    Running,
}

/// The service: owns its parts and drives them from the tick (UI task); the I/O task only runs the posted job's step. All state is zero at the start (it is a static in the service
/// file).
pub struct ProjectService {
    bound: BoundProject,
    queue: WorkQueue,
    stage: Stage,
    scan: SlotScan,
    run: TaskRun,
    clock: BusyClock,
    restore: ModuleRestore,
}

impl ProjectService {
    pub const fn new() -> ProjectService {
        ProjectService {
            bound: BoundProject::new(),
            queue: WorkQueue::new(),
            stage: Stage::Idle,
            scan: SlotScan::new(),
            run: TaskRun::new(),
            clock: BusyClock::new(),
            restore: ModuleRestore::new(),
        }
    }

    /// The slot of the current project, -1 for none.
    pub fn slot(&self) -> i32 {
        self.bound.slot().as_i32()
    }

    /// The live image: the current project's data.
    pub fn live_image(&self) -> &Image {
        self.bound.live_image()
    }

    /// `dk_project_status`.
    pub fn status(&self) -> u32 {
        (if self.bound.file().is_some() {
            ST_FILE
        } else {
            0
        }) | (if self.bound.is_dirty() { ST_DIRTY } else { 0 })
            | (if self.clock.is_deferred() {
                ST_DEFERRED
            } else {
                0
            })
            | (if self.bound.is_ignored() {
                ST_IGNORED
            } else {
                0
            })
    }

    /// The number of images installed so far.
    pub fn epoch(&self) -> u32 {
        self.bound.epoch()
    }

    /// The name of the current project (16 bytes as read), `E_NOT_FOUND` without one.
    pub fn current(&self) -> Result<(u32, [u8; 16]), i32> {
        self.bound.current()
    }

    // ---- the project scope (UI task) -------------------------------------------------------------------------------------------------

    /// `dk_settings_get` on the project scope: no current project is `E_SET_NOKEY`.
    pub fn scope_get(&self, id: &[u8], key: &[u8], out: &mut [u8]) -> i32 {
        self.bound.get(id, key, out)
    }

    /// `dk_settings_put`: no current project is `E_SET_OFF`; a put of the stored value changes nothing and sets no dirty flag.
    pub fn scope_put(&mut self, id: &[u8], key: &[u8], value: &[u8]) -> i32 {
        self.bound.put(id, key, value)
    }

    pub fn scope_remove(&mut self, id: &[u8], key: &[u8]) -> i32 {
        self.bound.remove(id, key)
    }

    // ---- the tick --------------------------------------------------------------------------------------------------------------------

    /// One UI pass: take the hooks' record, finish a job that ended, then advance the stage. Never blocks: a listing is polled, a write is a job.
    pub fn tick<E: Env + ?Sized>(&mut self, env: &mut E, pending: &HookRecord, job: &StoreJob) {
        let records = pending.take(env);
        if let Some(slot) = self.bound.start_slot(env, records.slot_set) {
            self.queue.add_load(slot);
        }
        self.queue.add(&records);
        self.restore.tick(env, &self.bound);
        if !self.take_job_end(env, job) {
            return;
        }
        if self.stage == Stage::Idle {
            self.retry_failed_load(env);
            self.start_next_work(env, job);
        } else if self.stage == Stage::Listing {
            self.advance_scan(env, job);
        } else {
            self.advance_run(env, job);
        }
    }

    /// Take the end of the posted job. `false` while the job still runs.
    fn take_job_end<E: Env + ?Sized>(&mut self, env: &mut E, job: &StoreJob) -> bool {
        let polled = job.poll();
        if let JobPoll::Running = polled {
            return false;
        }
        if let JobPoll::Ended(status) = polled {
            self.job_ended(env, status);
        }
        true
    }

    /// A failed load is asked again every `LOAD_RETRY_MS`, when no live operation stages.
    fn retry_failed_load<E: Env + ?Sized>(&mut self, env: &E) {
        if env.live_op() != 0 {
            return;
        }
        if let Some(slot) = self.bound.load_retry(env.now_ms()) {
            self.queue.add_retry(slot);
        }
    }

    /// Start the next piece of work the queue offers; work that was put back waits for its retry time.
    fn start_next_work<E: Env + ?Sized>(&mut self, env: &mut E, job: &StoreJob) {
        if self.clock.must_wait(env.now_ms()) {
            return;
        }
        let Some(work) = self.queue.next(&self.bound, env) else {
            return;
        };
        if let Some((scan_for, slot)) = work.scan() {
            self.scan.begin(scan_for, slot);
            self.stage = Stage::Listing;
        } else if let Work::Leave { setter_value } = work {
            self.bound.leave(env, job.lend(), setter_value);
        }
    }

    /// One step of the listing; when it ended, plan the work of the slot and start its tasks.
    fn advance_scan<E: Env + ?Sized>(&mut self, env: &mut E, job: &StoreJob) {
        let step = self.scan.step(env);
        if step == ScanStep::Waiting {
            return;
        }
        if step == ScanStep::Dropped {
            self.clock.answered();
            self.stage = Stage::Idle;
            return;
        }
        if let ScanStep::Refused(status) = step {
            self.scan_refused(env, status, job);
            return;
        }
        let result = match self.scan.finish(env, job.lend()) {
            Ok(result) => result,
            Err(status) => {
                self.scan_refused(env, status, job);
                return;
            }
        };
        self.clock.answered();
        self.run.begin();
        if result.scan_for() == ScanFor::Save && result.record().is_some() {
            self.bound.record_modules(env);
        }
        let plan = plan::plan(&result, &self.bound.view(), &mut self.run);
        self.stage = Stage::Idle;
        let Some(plan) = plan else {
            return;
        };
        if let Some(effect) = plan.effect {
            let applied = self.bound.apply(env, job.lend(), effect);
            if let Applied::Refused(status) = applied {
                // the read of the winner's file was refused after the listing found it: the same answer as a refused listing
                self.scan_refused(env, status, job);
                return;
            }
            if applied == Applied::Vanished {
                return;
            }
        }
        self.stage = Stage::Running;
    }

    /// The store refused the listing, or it failed half way, or a read of the finish was busy, with `status`. A busy store is asked again after `RETRY_MS`, for up to `DEFER_MS`;
    /// then the work gives up: one DEFERRED event per deferral (`BusyClock`). Any other status is an answer: it logs IGNORED with the store reason and ends the work. A load that
    /// ends leaves the project failed (`BoundProject::fail_load`: module writes go to the settings record only, the next save into the slot, setter call or retry loads it
    /// again) and warns, except for a slot that failed before: that refusal is not logged or warned again (it is a quiet retry or the same answer).
    fn scan_refused<E: Env + ?Sized>(&mut self, env: &mut E, status: i32, job: &StoreJob) {
        self.stage = Stage::Idle;
        let slot = self.scan.slot();
        let load = self.scan.scan_for() == ScanFor::Load;
        let repeat = load && self.bound.failed_load() == Some(slot);
        if is_busy(status) {
            let BusyVerdict::GiveUp { waited, first } = self.clock.note_busy(env.now_ms()) else {
                self.queue.requeue(&self.scan);
                return;
            };
            if first {
                env.log(
                    logk::PROJECT_DEFERRED,
                    [slot, status as u32, waited / 1000, 0, 0],
                );
            }
        } else if !repeat {
            env.log(
                logk::PROJECT_IGNORED,
                [slot, Ignore::Store as u32, status as u32, 0, 0],
            );
        }
        self.clock.answered();
        if load {
            let now = env.now_ms();
            self.bound.fail_load(env, job.lend(), slot, now);
            if !repeat {
                env.toast(Ignore::Store as u32);
            }
        }
    }

    /// Prepare the next task and post its job, or finish. A busy store waits for the retry time.
    fn advance_run<E: Env + ?Sized>(&mut self, env: &mut E, job: &StoreJob) {
        if self.clock.must_wait(env.now_ms()) {
            return;
        }
        let spec = loop {
            if self.run.finished() {
                self.clock.answered();
                self.run.log_end(env);
                self.stage = Stage::Idle;
                return;
            }
            let prepared = self
                .run
                .current_mut()
                .prepare(env, &mut self.bound, job.lend());
            if let Prepared::Job(spec) = prepared {
                break spec;
            }
            if let Prepared::Busy(status) = prepared {
                self.store_busy(env, status);
                return;
            }
            self.run.next_task();
        };
        if !job.post(env, spec) {
            // the job table was full: try the post again in a tenth of the retry time
            self.clock.hold_for(env.now_ms(), RETRY_MS / 10);
        }
    }

    /// The store answered busy to the task being run: the same task again after a second; after 30 s one event and the data stays dirty for the next save.
    fn store_busy<E: Env + ?Sized>(&mut self, env: &mut E, status: i32) {
        if let BusyVerdict::GiveUp { waited, first } = self.clock.note_busy(env.now_ms()) {
            if first {
                self.run.log_deferred(env, status, waited);
            }
            self.run.abort(status);
        }
    }

    /// The job of the task being run ended with `status`.
    fn job_ended<E: Env + ?Sized>(&mut self, env: &mut E, status: i32) {
        if self.stage != Stage::Running {
            return;
        }
        if status == dkrt_sdk::OK {
            if self.run.task_done(&mut self.bound) {
                self.clock.clear_deferred();
            }
            self.clock.answered();
            self.run.next_task();
        } else if is_busy(status) {
            self.store_busy(env, status);
        } else {
            // any other status ends the attempt: no retry loop; the data stays dirty
            self.run.abort(status);
        }
    }

    /// Is there work in progress (a stage that is not idle, or a job that was not taken)? For a host test that drives the service by hand.
    pub fn busy(&self, job: &StoreJob) -> bool {
        self.stage != Stage::Idle || job.is_posted() || self.restore.busy()
    }
}

/// Byte slices are equal (a loop, not `memcmp`, on the ColdFire).
fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut at = 0;
    while at < a.len() {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}
