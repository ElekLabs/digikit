//! `Env` of the engine for `DkHost`: the engine's environment over the runtime's core, the firmware's slot records and the SDK imports (the UI task).
use super::c_strings::{cpath, cstr};
use super::firmware_project::{
    firmware_mmcfs, firmware_slot, REC_BASE, REC_NAME, REC_SIZE, REC_STAMP,
};
use super::host::DkHost;
use super::sdk_imports as sys;
use super::sdk_imports::{FILE_KIND, POLL_END, POLL_ROW, STORE};
use super::toast::{show, show_name};
use core::ptr::{addr_of, read_volatile};
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_svc_project::dkp::Ignore;
use dkrt_svc_project::engine::{
    stable_record, Env, ListPoll, ListRow, LiveJob, LoadedModule, ModuleId, ModulePath,
    ModuleWarning, SlotRecord,
};
use dkrt_svc_rt::ImportCore;

const CORE: ImportCore = ImportCore;

impl Env for DkHost {
    fn device(&self) -> u16 {
        CORE.device_id() as u16
    }
    fn live_op(&self) -> u32 {
        CORE.live_op()
    }
    fn now_ms(&self) -> u32 {
        // the block clock runs at 1500 Hz; without a counter the UI passes (about 120 a second) stand in
        // SAFETY: plain words, written in the UI task only.
        unsafe {
            let b = sys::dk_time_blocks();
            if b != 0 {
                b / 3 * 2
            } else {
                read_volatile(addr_of!(self.passes)).wrapping_mul(8)
            }
        }
    }
    fn irq_mask(&self) -> u32 {
        CORE.irq_mask()
    }
    fn irq_restore(&self, sr: u32) {
        CORE.irq_restore(sr)
    }
    fn firmware_slot(&self) -> Option<u32> {
        firmware_slot()
    }
    fn slot_record(&self, slot: u32) -> Option<SlotRecord> {
        if slot > 0x80 {
            return None;
        }
        // SAFETY: the MmcFs object is the firmware's; the layout is the constants above. The firmware refreshes the record under a mutex; the service reads it from the UI task tick.
        unsafe {
            let fs = firmware_mmcfs();
            if fs == 0 {
                return None;
            }
            let r = (fs + REC_BASE + slot * REC_SIZE) as usize;
            // no lock: the record is read twice and used only when both reads agree
            stable_record(|| {
                if read_volatile(r as *const u8) == 0 {
                    return None;
                }
                let mut name = [0u8; 16];
                let mut i = 0;
                while i < 16 {
                    name[i] = read_volatile((r + REC_NAME + i) as *const u8);
                    i += 1;
                }
                Some(SlotRecord {
                    name,
                    stamp: read_volatile((r + REC_STAMP) as *const u32),
                })
            })
        }
    }
    fn list_start(&mut self) -> i32 {
        // SAFETY: the handle was opened in `init`.
        unsafe {
            let t = sys::dk_files_list(
                read_volatile(addr_of!(self.files)),
                STORE,
                b"projects\0".as_ptr(),
                None,
                core::ptr::null_mut(),
            );
            if t > 0 {
                self.ticket = t as u32;
                return 0;
            }
            if t == 0 {
                return dkrt_sdk::E_NOT_FOUND;
            }
            t
        }
    }
    fn list_next(&mut self, row: &mut ListRow) -> ListPoll {
        // SAFETY: `self.entry` is this file's own buffer of one `dk_file_entry` (304 bytes).
        unsafe {
            loop {
                let r = sys::dk_files_poll(
                    read_volatile(addr_of!(self.files)),
                    self.ticket,
                    self.entry.0.as_mut_ptr(),
                    304,
                );
                if r == 0 {
                    return ListPoll::Pending;
                }
                if r == POLL_END {
                    return ListPoll::End;
                }
                if r != POLL_ROW {
                    // an error of the listing: the folder is not there or the store is off
                    return if r == dkrt_sdk::E_NOT_FOUND {
                        ListPoll::End
                    } else {
                        ListPoll::Err(r)
                    };
                }
                let e = &self.entry;
                let kind = u32::from_be_bytes([e.0[0], e.0[1], e.0[2], e.0[3]]);
                if kind != FILE_KIND {
                    continue;
                }
                let path = &e.0[48..];
                let mut n = 0;
                while n < path.len() && path[n] != 0 {
                    n += 1;
                }
                if n > row.bytes.len() {
                    continue;
                }
                row.bytes[..n].copy_from_slice(&path[..n]);
                row.len = n;
                return ListPoll::Row;
            }
        }
    }
    fn read(&mut self, path: &[u8], buf: &mut [u8]) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: `buf` is the image's own buffer; the handle was opened in `init`.
        unsafe {
            sys::dk_files_read(
                read_volatile(addr_of!(self.files)),
                cpath(path, &mut c),
                buf.as_mut_ptr(),
                buf.len() as u32,
            )
        }
    }
    fn post(&mut self) -> i32 {
        // SAFETY: the step and the done function are this file's; the job reaches the state through the statics.
        unsafe {
            sys::dk_io_post(
                Some(super::files::io_step),
                Some(super::files::io_done),
                core::ptr::null_mut(),
            )
        }
    }
    fn log(&mut self, kind: u16, args: [u32; 5]) {
        // SAFETY: the log of the runtime.
        unsafe { CORE.log(kind, LOG_INFO, args) }
    }
    fn toast(&mut self, reason: u32) {
        // one short line for a file that is not used (docs/design/digikit-project.md section 6); the facts are in the log (PROJECT_IGNORED)
        show(
            self.ui,
            &[if reason == Ignore::Store as u32 {
                b"PROJECT DATA NOT READ"
            } else {
                b"PROJECT DATA IGNORED"
            }],
        );
    }
    fn record_get(&mut self, key: &[u8], out: &mut [u8]) -> i32 {
        // SAFETY: the settings record of the runtime (device scope), through the core.
        unsafe { CORE.settings_get(key, out) }
    }
    fn changed(&mut self, owner: &[u8]) {
        // SAFETY: the runtime reloads the module of that id when it opened its settings.
        unsafe { CORE.settings_changed(owner) }
    }
    fn loaded_module(&self, record: usize) -> Option<LoadedModule> {
        let owner = (record + 1) as u8;
        // SAFETY: the runtime's module records: the path is null for a record with no module, else NUL terminated; the id is the record's own name (31 bytes and a NUL).
        unsafe {
            let path = CORE.module_path(owner);
            if path.is_null() {
                return None;
            }
            let name = CORE.module_name(owner);
            Some(LoadedModule {
                id: ModuleId::new(cstr(name, 31))?,
                path: ModulePath::new(cstr(path, 63))?,
            })
        }
    }
    fn store_hash(&self, path: &[u8]) -> u32 {
        // SAFETY: the runtime's store, through the core.
        match unsafe { CORE.storage_stat(STORE, path) } {
            Ok(stat) if stat.hash_valid => {
                u32::from_be_bytes([stat.hash[0], stat.hash[1], stat.hash[2], stat.hash[3]])
            }
            _ => 0,
        }
    }
    fn find_module_file(&mut self, file: &[u8]) -> Result<ModulePath, i32> {
        let mut found = [0u8; 64];
        // SAFETY: the runtime lists the store's module files.
        let n = unsafe { CORE.store_find(b"modules/", file, &mut found) };
        if n < 0 {
            return Err(n);
        }
        ModulePath::new(&found[..n as usize]).ok_or(dkrt_sdk::E_NOT_FOUND)
    }
    fn live_load(&mut self, path: &ModulePath) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: the live job of the runtime (the UI task); the path is NUL terminated.
        unsafe { CORE.live(dkrt_sdk::LIVE_LOAD, cpath(path.as_bytes(), &mut c)) }
    }
    fn live_job(&self) -> LiveJob {
        // SAFETY: the live job's state.
        let job = unsafe { CORE.live_state() };
        LiveJob {
            phase: job.phase,
            result: job.result,
            op: job.op,
        }
    }
    fn selection_add(&mut self, path: &ModulePath) -> i32 {
        // SAFETY: the saved selection of the runtime.
        unsafe { CORE.selection_add(path.as_bytes()) }
    }
    fn warn_module(&mut self, warning: ModuleWarning, id: &ModuleId) {
        // the module's own name, as the menu shows it; `PROJECT_MODULE` carries the facts
        match warning {
            ModuleWarning::Missing => show(self.ui, &[b"MODULE MISSING: ", id.name()]),
            ModuleWarning::NoFit => show_name(self.ui, id.name(), b" DOES NOT FIT"),
            ModuleWarning::Refused => show_name(self.ui, id.name(), b" NOT LOADED"),
            ModuleWarning::NotKept => show_name(self.ui, id.name(), b" NOT KEPT"),
        }
    }
}
