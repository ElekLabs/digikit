//! The file operations of one piece of work: [`Task`] is one operation, [`TaskRun`] holds the list and the state of its run. UI task only (the tasks' jobs run in the I/O task).
use super::writer::JobSpec;
use super::{is_busy, BoundProject, Env, SlotRecord};
use crate::dkp::{self, Image, AREA};
use crate::name::{self, Path};
use dkrt_sdk::logk;
use dkrt_sdk::{E_NOT_FOUND, OK};

/// The most queued file operations of one piece of work.
pub const TASK_MAX: usize = 9;

/// A queued file operation of one piece of work.
#[derive(Clone, Copy)]
#[repr(u8)] // zero first: the service is a bss static
pub enum Task {
    /// An unused entry of the list.
    Empty,
    /// Move the file at `from` to the archive: write its copy (named by its own header), then remove `from`. `to` is the archive path, chosen at the first preparation and kept
    /// for every later one (a busy retry).
    Archive { from: Path, to: Option<Path> },
    /// Write the live image as `to` for slot `slot` with the record `record` and write counter `counter`; then remove `superseded` (superseded copies).
    WriteLive {
        to: Path,
        slot: u8,
        record: SlotRecord,
        counter: u32,
        superseded: [Path; 3],
        superseded_count: u8,
    },
    /// Remove a superseded copy.
    Delete { path: Path },
    /// Copy the file at `from` to `to` for slot `slot` with the record `record` (counter 1).
    Copy {
        from: Path,
        to: Path,
        slot: u8,
        record: SlotRecord,
    },
}

/// What preparing a task gave.
#[allow(clippy::large_enum_variant)] // a value on the stack for one call
pub enum Prepared {
    /// The job to post.
    Job(JobSpec),
    /// There is nothing to do (a file that vanished, or that cannot be archived): the next task.
    Skip,
    /// The store answered busy with this status while the task was prepared: the same task again later.
    Busy(i32),
}

/// Where a file goes in the archive.
enum ArchiveTarget {
    /// This path holds no file.
    Free(Path),
    /// A file of the same content (write counter and crc32) is archived already: the source only needs to go.
    Already,
    /// All `ARCHIVE_COPIES_MAX` paths of the name and stamp hold other files.
    Full,
    /// The store was busy answering a probe with this status.
    Busy(i32),
}

/// Choose the archive path for the file of `name` and `stamp` whose header identity is `source`: the first path of the name and stamp that holds no file; a file at a path
/// with the source's content counts as the archive copy. A path is free only when the read says `E_NOT_FOUND`; a busy store is no answer (never "taken"); any other file or
/// status means the path is taken, so a path is skipped, never replaced. A candidate counts as the archive copy only when the whole file passes `dkp::check` and has the source's identity (write
/// counter and crc32): a header alone could match a truncated or corrupt copy, and the source would be removed. The probes read whole files into `buffer`, like every read of the device.
fn archive_target<E: Env + ?Sized>(
    env: &mut E,
    buffer: &mut Image,
    name: &[u8],
    stamp: u32,
    source: (u32, u32),
) -> ArchiveTarget {
    let mut copy = 1;
    while copy <= name::ARCHIVE_COPIES_MAX {
        let path = name::archive_path_copy(name, stamp, copy);
        let size = env.read(path.as_bytes(), &mut buffer.0);
        if size == E_NOT_FOUND {
            return ArchiveTarget::Free(path);
        }
        if is_busy(size) {
            return ArchiveTarget::Busy(size);
        }
        if size >= 0
            && size as usize <= AREA
            && dkp::check(&buffer.0[..size as usize], env.device()).is_ok()
            && dkp::identity(&buffer.0[..size as usize]) == source
        {
            return ArchiveTarget::Already;
        }
        copy += 1;
    }
    ArchiveTarget::Full
}

impl Task {
    /// Fill the job for this task: the bytes it writes go to `buffer`; a `WriteLive` freezes the bound project's image into it. (If-let chains, not a `match`: a match of this size
    /// compiles to a jump table, whose words the ColdFire ISA check of the packer reads as instructions.)
    pub fn prepare<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        bound: &mut BoundProject,
        buffer: &mut Image,
    ) -> Prepared {
        let mut spec = JobSpec::new();
        if let Task::Archive { from, to } = self {
            let size = env.read(from.as_bytes(), &mut buffer.0);
            if is_busy(size) {
                return Prepared::Busy(size);
            }
            if size < 0 || size as usize > AREA {
                return Prepared::Skip;
            }
            if to.is_none() {
                // an archive file is never replaced: a name and stamp that were archived before take the next free suffix; with no free one the file stays where it is
                let head = dkp::head_of(&buffer.0);
                let source = dkp::identity(&buffer.0[..size as usize]);
                match archive_target(env, buffer, &head.name, head.stamp, source) {
                    ArchiveTarget::Free(path) => *to = Some(path),
                    ArchiveTarget::Already => {
                        spec.delete(*from);
                        return Prepared::Job(spec);
                    }
                    ArchiveTarget::Full => return Prepared::Skip,
                    ArchiveTarget::Busy(status) => return Prepared::Busy(status),
                }
                // the probes overwrote the buffer: the source again
                let again = env.read(from.as_bytes(), &mut buffer.0);
                if is_busy(again) {
                    return Prepared::Busy(again);
                }
                if again != size {
                    return Prepared::Skip;
                }
            }
            let Some(path) = *to else {
                return Prepared::Skip;
            };
            spec.write(path, size as u32);
            spec.delete(*from);
            return Prepared::Job(spec);
        }
        if let Task::WriteLive {
            to,
            slot,
            record,
            counter,
            superseded,
            superseded_count,
        } = *self
        {
            let used = bound.freeze_into(env, buffer, slot, &record, counter);
            spec.write(to, used);
            let mut at = 0;
            while at < superseded_count as usize {
                spec.delete(superseded[at]);
                at += 1;
            }
            return Prepared::Job(spec);
        }
        if let Task::Delete { path } = *self {
            spec.delete(path);
            return Prepared::Job(spec);
        }
        if let Task::Copy {
            from,
            to,
            slot,
            record,
        } = *self
        {
            let size = env.read(from.as_bytes(), &mut buffer.0);
            if size < 0 || dkp::check(&buffer.0[..(size as usize).min(AREA)], env.device()).is_err()
            {
                return Prepared::Skip;
            }
            let mut head = dkp::head_of(&buffer.0);
            head.slot = slot;
            head.name = record.name;
            head.name_len = name::trimmed_len_raw(&record.name) as u8;
            head.stamp = record.stamp;
            dkp::set_head(&mut buffer.0, &head);
            dkp::seal(&mut buffer.0, 1);
            spec.write(to, head.used);
            return Prepared::Job(spec);
        }
        Prepared::Skip
    }
}

/// What a save reports when its run ended: the SAVED event.
#[derive(Clone, Copy)]
pub struct SaveEnd {
    pub slot: u32,
    /// The bytes of the file.
    pub bytes: u32,
    /// Its write counter.
    pub counter: u32,
}

/// Owns the tasks of one piece of work and where their run is: the planner fills it in place (`begin`, `push`), the tick prepares and posts the task at `at`. Reused for each piece
/// of work; its list holds stale entries beyond `count`.
pub struct TaskRun {
    tasks: [Task; TASK_MAX],
    count: usize,
    at: usize,
    /// A save: what to log when the run ended.
    save_end: Option<SaveEnd>,
    /// The status that ended the run: `OK`, or the last status of a write that gave up.
    result: i32,
}

impl TaskRun {
    pub const fn new() -> TaskRun {
        TaskRun {
            tasks: [Task::Empty; TASK_MAX],
            count: 0,
            at: 0,
            save_end: None,
            result: OK,
        }
    }

    /// Start a new list (in place): no tasks, no SAVED event.
    pub fn begin(&mut self) {
        self.count = 0;
        self.at = 0;
        self.save_end = None;
        self.result = OK;
    }
    pub fn push(&mut self, task: Task) {
        if self.count < TASK_MAX {
            self.tasks[self.count] = task;
            self.count += 1;
        }
    }
    /// The run is a save: log SAVED with these facts when it ended.
    pub fn set_save_end(&mut self, end: SaveEnd) {
        self.save_end = Some(end);
    }

    pub fn finished(&self) -> bool {
        self.at >= self.count
    }
    pub fn current(&self) -> &Task {
        &self.tasks[self.at]
    }
    pub fn current_mut(&mut self) -> &mut Task {
        &mut self.tasks[self.at]
    }
    pub fn next_task(&mut self) {
        self.at += 1;
    }
    /// End the run with `status`: the tasks that are left do not run.
    pub fn abort(&mut self, status: i32) {
        self.result = status;
        self.at = self.count;
    }

    /// The job of the current task ended well: tell the bound project what that means for it. `true` when the live image was written.
    pub fn task_done(&self, bound: &mut BoundProject) -> bool {
        // a save that archived the stale file of the current slot: that file is no longer the reason for `ignored`
        if let (Task::Archive { .. }, Some(end)) = (self.current(), self.save_end) {
            if bound.slot().index() == Some(end.slot) {
                bound.clear_ignored();
            }
        }
        if let Task::WriteLive {
            to,
            slot,
            record,
            counter,
            ..
        } = *self.current()
        {
            bound.mark_written(to, slot, &record, counter);
            return true;
        }
        false
    }

    /// The run ended (all its tasks ran, or it was aborted): a save logs SAVED.
    pub fn log_end<E: Env + ?Sized>(&self, env: &mut E) {
        if let Some(end) = self.save_end {
            env.log(
                logk::PROJECT_SAVED,
                [end.slot, end.bytes, end.counter, self.result as u32, 0],
            );
        }
    }

    /// The store was busy for `waited` ms with `status`: DEFERRED names the slot of a save (0 for other work).
    pub fn log_deferred<E: Env + ?Sized>(&self, env: &mut E, status: i32, waited: u32) {
        let slot = self.save_end.map(|end| end.slot).unwrap_or(0);
        env.log(
            logk::PROJECT_DEFERRED,
            [slot, status as u32, waited / 1000, 0, 0],
        );
    }
}

impl Default for TaskRun {
    fn default() -> Self {
        Self::new()
    }
}
