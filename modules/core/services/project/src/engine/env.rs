//! What the service reaches outside itself: the traits [`Env`] (UI task) and [`Files`] (I/O task) and the plain values they pass. The host tests give fakes for both.
/// A record of the firmware's project list: the name as the listing read it (16 bytes) and the save stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRecord {
    pub name: [u8; 16],
    pub stamp: u32,
}

/// The longest path of one listing row.
const LIST_ROW_MAX: usize = 64;

/// One row of the store listing: the whole path.
pub struct ListRow {
    pub bytes: [u8; LIST_ROW_MAX],
    pub len: usize,
}

impl ListRow {
    pub const fn new() -> ListRow {
        ListRow {
            bytes: [0; LIST_ROW_MAX],
            len: 0,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Default for ListRow {
    fn default() -> Self {
        Self::new()
    }
}

/// The answer of one poll of the listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListPoll {
    Pending,
    /// The next row (a file; the folder rows are skipped by the caller) is in `row`.
    Row,
    End,
    Err(i32),
}

use super::restore::{LiveJob, LoadedModule, ModuleId, ModulePath, ModuleWarning};

/// What the tick reaches (UI task).
pub trait Env {
    /// The `[digikit] id` of the device.
    fn device(&self) -> u16;
    /// The module that a live operation changes (0: none): a read waits while one runs (the read stage is shared with the loader).
    fn live_op(&self) -> u32;
    /// A clock in milliseconds (wraps).
    fn now_ms(&self) -> u32;
    fn irq_mask(&self) -> u32;
    fn irq_restore(&self, sr: u32);
    /// The firmware's record of project `slot` (0 to 128, 128 is the live mirror): `None` when the slot has none or the firmware is not bound.
    fn slot_record(&self, slot: u32) -> Option<SlotRecord>;
    /// The slot of the project that the firmware has current now (0 to 127): `None` for no project, or when the firmware is not bound. The service reads it until it knows the
    /// current project at its start, because the stock setter is not called for a project that was already current.
    fn firmware_slot(&self) -> Option<u32> {
        None
    }
    /// Start the listing of `projects/`; a negative status is a refusal (`E_NOT_FOUND`: the folder does not exist).
    fn list_start(&mut self) -> i32;
    fn list_next(&mut self, row: &mut ListRow) -> ListPoll;
    /// Read a whole store file into `buf`: its size, or a negative status.
    fn read(&mut self, path: &[u8], buf: &mut [u8]) -> i32;
    /// Post the write job: the runtime calls `ProjectService::io_step` in the I/O task and `ProjectService::io_done` in the UI task. `DK_E_BUSY` when the job table is full.
    fn post(&mut self) -> i32;
    fn log(&mut self, kind: u16, args: [u32; 5]);
    /// One toast for a file that is not used (`reason` as `Ignore`).
    fn toast(&mut self, reason: u32);
    /// The value of the settings record (device scope) under the stored key `key` (`<module id>/<key>`): its length, or a negative status. What a module reads when the
    /// current project holds no value of its own.
    fn record_get(&mut self, key: &[u8], out: &mut [u8]) -> i32;
    /// The content module loaded in the runtime's module record `record` (0 to `MODULES_MAX - 1`), `None` for a record with no module.
    fn loaded_module(&self, record: usize) -> Option<LoadedModule>;
    /// The store hash of the file at `path` (the four bytes of the table's hash slot, big-endian), 0 when the store has none.
    fn store_hash(&self, path: &[u8]) -> u32;
    /// The store path of the module file `file` (`<author>-<name>.dkm`, under `modules/` at any depth): `E_NOT_FOUND` for none, `E_BUSY` when the store cannot be listed now.
    fn find_module_file(&mut self, file: &[u8]) -> Result<ModulePath, i32>;
    /// Queue the live load of the module file at `path`, as DIGIKIT > MODULES does (`dkrt_live` with `LIVE_LOAD`): its status.
    fn live_load(&mut self, path: &ModulePath) -> i32;
    /// The state of the runtime's live job.
    fn live_job(&self) -> LiveJob;
    /// Add the module file at `path` to the saved selection: its status.
    fn selection_add(&mut self, path: &ModulePath) -> i32;
    /// One short warning that module `id` of the project's list is not loaded.
    fn warn_module(&mut self, warning: ModuleWarning, id: &ModuleId);
    /// The effective settings of module `owner` differ between the project that was current and the one that is now (the runtime reloads the module when it opened its settings).
    fn changed(&mut self, owner: &[u8]);
}

/// What the write job reaches (I/O task): the store calls of <dk/platform/files.h>.
pub trait Files {
    /// `dk_files_create(DK_FILES_STORE, path, size, NULL, DK_FILE_REPLACE)`: the handle or a negative status.
    fn create(&mut self, path: &[u8], size: u32) -> i32;
    /// `dk_files_write`: one call with all the bytes; `E_BUSY` only before it wrote anything.
    fn write(&mut self, handle: i32, buf: &[u8]) -> i32;
    fn close(&mut self, handle: i32) -> i32;
    fn abort(&mut self, handle: i32);
    fn delete(&mut self, path: &[u8]) -> i32;
}
