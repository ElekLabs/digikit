#![allow(dead_code)] // each test binary uses a part of it
//! The host of the owner in host tests: `Env` over one project in the firmware list, no files, and a settings overlay table of its own. Shared by `contract.rs` and `state.rs`.
use core::cell::Cell;
use dkrt_abi::provider::Status;
use dkrt_abi::service::{SettingsOverlay, SETTINGS_OVERLAY_VERSION};
use dkrt_svc_project::engine::{
    Env, Files, ListPoll, ListRow, LiveJob, LoadedModule, ModuleId, ModulePath, ModuleWarning,
    Pending, SlotRecord, StoreJob,
};
use dkrt_svc_project::project::Host;

std::thread_local! {
    /// The next `open` fails with this status (a test sets it before `start`).
    pub static OPEN_FAILS: Cell<Status> = const { Cell::new(0) };
    /// The hooks' record and the write job's cell of this test thread (a static of the file in the service).
    pub static HOOKS: &'static Pending = Box::leak(Box::new(Pending::new()));
    pub static JOB: &'static StoreJob = Box::leak(Box::new(StoreJob::new()));
    /// What `post` answers: a negative status is a full job table.
    pub static POST_STATUS: Cell<i32> = const { Cell::new(0) };
}

/// A host with one project in the firmware list and no store: enough to make a project current, and no SDK. All-zero bytes are its initial state (it has none).
pub struct TestHost;
impl Env for TestHost {
    fn device(&self) -> u16 {
        1
    }
    fn live_op(&self) -> u32 {
        0
    }
    fn now_ms(&self) -> u32 {
        0
    }
    fn irq_mask(&self) -> u32 {
        0
    }
    fn irq_restore(&self, _: u32) {}
    fn slot_record(&self, slot: u32) -> Option<SlotRecord> {
        (slot == 3).then(|| SlotRecord {
            name: *b"CONTRACT\0\0\0\0\0\0\0\0",
            stamp: 7,
        })
    }
    fn list_start(&mut self) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn list_next(&mut self, _: &mut ListRow) -> ListPoll {
        ListPoll::End
    }
    fn read(&mut self, _: &[u8], _: &mut [u8]) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn post(&mut self) -> i32 {
        POST_STATUS.with(|s| s.get())
    }
    fn log(&mut self, _: u16, _: [u32; 5]) {}
    fn toast(&mut self, _: u32) {}
    fn record_get(&mut self, _: &[u8], _: &mut [u8]) -> i32 {
        dkrt_sdk::proto::E_SET_NOKEY
    }
    fn changed(&mut self, _: &[u8]) {}
    fn loaded_module(&self, _: usize) -> Option<LoadedModule> {
        None
    }
    fn store_hash(&self, _: &[u8]) -> u32 {
        0
    }
    fn find_module_file(&mut self, _: &[u8]) -> Result<ModulePath, i32> {
        Err(dkrt_sdk::E_NOT_FOUND)
    }
    fn live_load(&mut self, _: &ModulePath) -> i32 {
        dkrt_sdk::E_BUSY
    }
    fn live_job(&self) -> LiveJob {
        LiveJob {
            phase: 0,
            result: 0,
            op: 0,
        }
    }
    fn selection_add(&mut self, _: &ModulePath) -> i32 {
        0
    }
    fn warn_module(&mut self, _: ModuleWarning, _: &ModuleId) {}
}

/// The write job's file calls: none are reached.
#[derive(Clone, Copy)]
pub struct NoFiles;
impl Files for NoFiles {
    fn create(&mut self, _: &[u8], _: u32) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn write(&mut self, _: i32, _: &[u8]) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn close(&mut self, _: i32) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn abort(&mut self, _: i32) {}
    fn delete(&mut self, _: &[u8]) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
}

unsafe extern "C" fn get(_: *const u8, _: u32, _: *const u8, _: u32, _: *mut u8, _: u32) -> i32 {
    dkrt_sdk::proto::E_SET_NOKEY
}
unsafe extern "C" fn put(_: *const u8, _: u32, _: *const u8, _: u32, _: *const u8, _: u32) -> i32 {
    dkrt_sdk::proto::E_SET_OFF
}
unsafe extern "C" fn remove(_: *const u8, _: u32, _: *const u8, _: u32) -> i32 {
    dkrt_sdk::proto::E_SET_OFF
}
pub static TABLE: SettingsOverlay = SettingsOverlay {
    version: SETTINGS_OVERLAY_VERSION,
    get,
    put,
    remove,
};

// SAFETY: a zero-sized type.
unsafe impl Host for TestHost {
    fn open(&mut self) -> Result<(), Status> {
        match OPEN_FAILS.with(|f| f.get()) {
            0 => Ok(()),
            status => Err(status),
        }
    }
    fn begin_pass(&mut self) {}
    fn hook_record(&self) -> &'static Pending {
        HOOKS.with(|h| *h)
    }
    fn job(&self) -> &'static StoreJob {
        JOB.with(|j| *j)
    }
    fn overlay() -> *const SettingsOverlay {
        &TABLE
    }
}
