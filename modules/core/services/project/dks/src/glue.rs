//! The service file's glue: the SDK imports the engine runs over (`DkEnv` in the UI task, `DkFiles` in the I/O task), the state, the tick, the write job's entries and the table
//! that the runtime's settings overlay forwards to. All state is static: one service, one file.
//!
//! A host build (the assembly test of the runtime runs the declarations of `service!`) has no SDK imports to link: the calls are stubs with the answer of a missing service.
use crate::hooks::DKRT_PROJECT_PENDING;
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_abi::service::{SettingsOverlay, SETTINGS_OVERLAY_VERSION};
use dkrt_svc_project::dkp::Ignore;
use dkrt_svc_project::engine::{
    Env, Files, ListPoll, ListRow, LiveJob, LoadedModule, ModuleId, ModulePath, ModuleWarning,
    ProjectService, SlotRecord, stable_record,
};
use dkrt_svc_rt::ImportCore;

const CORE: ImportCore = ImportCore;
/// `DK_FILES_STORE` and `DK_FILE_REPLACE` (<dk/platform/files.h>).
const STORE: u32 = 2;
const REPLACE: u32 = 1;
const FILE_KIND: u32 = 1;
/// The listing rows of `dk_files_poll`: DK_FILES_PENDING 0, ROW 1, END 2.
const POLL_ROW: i32 = 1;
const POLL_END: i32 = 2;
const E_MISSING: i32 = -73;

#[cfg(target_arch = "m68k")]
mod sys {
    pub use dkrt_sdk::api::{IoDoneFn, IoStepFn};
    extern "C" {
        pub fn dk_files_open(ctx: *mut u8, out: *mut *mut u8) -> i32;
        pub fn dk_files_list(
            f: *mut u8,
            root: u32,
            path: *const u8,
            notify: Option<dkrt_sdk::api::FilesEventFn>,
            arg: *mut u8,
        ) -> i32;
        pub fn dk_files_poll(f: *mut u8, ticket: u32, out: *mut u8, capacity: u32) -> i32;
        pub fn dk_files_read(files: *mut u8, path: *const u8, buf: *mut u8, cap: u32) -> i32;
        pub fn dk_files_create(
            f: *mut u8,
            root: u32,
            path: *const u8,
            size: u32,
            hash4: *const u8,
            flags: u32,
        ) -> i32;
        pub fn dk_files_write(f: *mut u8, h: i32, offset: u32, buf: *const u8, len: u32) -> i32;
        pub fn dk_files_close(f: *mut u8, h: i32, hash_out: *mut u8) -> i32;
        pub fn dk_files_abort(f: *mut u8, h: i32) -> i32;
        pub fn dk_files_delete(f: *mut u8, root: u32, path: *const u8) -> i32;
        pub fn dk_io_post(step: Option<IoStepFn>, done: Option<IoDoneFn>, arg: *mut u8) -> i32;
        pub fn dk_time_blocks() -> u32;
        // optional imports of core/ui (`requires` in sdk/sdk.toml): the loader binds them to the absent stub when the image has no core/ui
        pub fn dk_ui_open(ctx: *mut u8, out: *mut *mut u8) -> i32;
        pub fn dk_ui_toast(ui: *mut u8, text: *const u8) -> i32;
    }
    // each import has the type the SDK declares
    const _: dkrt_sdk::api::DkFilesOpenFn = dk_files_open;
    const _: dkrt_sdk::api::DkFilesListFn = dk_files_list;
    const _: dkrt_sdk::api::DkFilesPollFn = dk_files_poll;
    const _: dkrt_sdk::api::DkFilesReadFn = dk_files_read;
    const _: dkrt_sdk::api::DkFilesCreateFn = dk_files_create;
    const _: dkrt_sdk::api::DkFilesWriteFn = dk_files_write;
    const _: dkrt_sdk::api::DkFilesCloseFn = dk_files_close;
    const _: dkrt_sdk::api::DkFilesAbortFn = dk_files_abort;
    const _: dkrt_sdk::api::DkFilesDeleteFn = dk_files_delete;
    const _: dkrt_sdk::api::DkIoPostFn = dk_io_post;
    const _: dkrt_sdk::api::DkTimeBlocksFn = dk_time_blocks;
    const _: dkrt_sdk::api::DkUiOpenFn = dk_ui_open;
    const _: dkrt_sdk::api::DkUiToastFn = dk_ui_toast;
}

#[cfg(not(target_arch = "m68k"))]
mod sys {
    pub type IoStepFn = unsafe extern "C" fn(*mut u8) -> i32;
    pub type IoDoneFn = unsafe extern "C" fn(*mut u8, i32);
    const MISSING: i32 = super::E_MISSING;
    pub unsafe fn dk_files_open(_: *mut u8, _: *mut *mut u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_list(
        _: *mut u8,
        _: u32,
        _: *const u8,
        _: Option<unsafe extern "C" fn(u32, u32, i32, *mut u8)>,
        _: *mut u8,
    ) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_poll(_: *mut u8, _: u32, _: *mut u8, _: u32) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_read(_: *mut u8, _: *const u8, _: *mut u8, _: u32) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_create(
        _: *mut u8,
        _: u32,
        _: *const u8,
        _: u32,
        _: *const u8,
        _: u32,
    ) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_write(_: *mut u8, _: i32, _: u32, _: *const u8, _: u32) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_close(_: *mut u8, _: i32, _: *mut u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_abort(_: *mut u8, _: i32) -> i32 {
        MISSING
    }
    pub unsafe fn dk_files_delete(_: *mut u8, _: u32, _: *const u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_io_post(_: Option<IoStepFn>, _: Option<IoDoneFn>, _: *mut u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_time_blocks() -> u32 {
        0
    }
    pub unsafe fn dk_ui_open(_: *mut u8, _: *mut *mut u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_ui_toast(_: *mut u8, _: *const u8) -> i32 {
        MISSING
    }
}

/// `MmcFs *f(void)` of the firmware (`cf.project.fn.mmcfs`); 0 on a host build and when the port has no such name.
#[cfg(target_arch = "m68k")]
unsafe fn firmware_mmcfs() -> u32 {
    if core::hint::black_box(dkfw::project::mmcfs as usize) == 0 {
        0
    } else {
        dkfw::project::mmcfs()
    }
}
#[cfg(not(target_arch = "m68k"))]
unsafe fn firmware_mmcfs() -> u32 {
    0
}

/// The listing record of a slot in the firmware's `MmcFs` object (finding 110, `cf.project.fn.mmcfs` in devices/fw/project.toml; the same on both devices): the record of
/// slot s is at `MmcFs + REC_BASE + s * REC_SIZE`; byte 0 is nonzero when the slot exists (the firmware's own test), the 16 name bytes are at `REC_NAME`, the save stamp
/// (u32) at `REC_STAMP`.
const REC_BASE: u32 = 0x10;
const REC_SIZE: u32 = 0x1c;
const REC_NAME: usize = 8;
const REC_STAMP: usize = 0x18;

/// The slot of the current project as the firmware stores it (`cf.project.value.slot_byte`: one byte, -1 for none); `None` on a host build and for no project. The byte holds the
/// last persisted slot, read before the stock project load (devices/fw/project.toml). The extern is declared here and not in `dkfw` (the value is `codegen = false`), and
/// `[object.externs]` of `dks/module.toml` names it, as it names the hook tails: `patchkit pack` makes it the import `dk_fw_<hash>`, which the loader resolves from the
/// port file. The address is 0 only when the port marks the name absent for the firmware (the loader accepts the file and the read is off).
#[cfg(target_arch = "m68k")]
fn firmware_slot() -> Option<u32> {
    extern "C" {
        static project_slot_byte: u8;
    }
    // SAFETY: the address is the firmware's variable, resolved by the port file; 0 when the port marks it absent.
    unsafe {
        let at = core::hint::black_box(addr_of!(project_slot_byte) as usize);
        if at == 0 {
            return None;
        }
        let slot = read_volatile(at as *const u8) as i8;
        if slot >= 0 {
            Some(slot as u32)
        } else {
            None
        }
    }
}
#[cfg(not(target_arch = "m68k"))]
fn firmware_slot() -> Option<u32> {
    None
}

static mut PROJECT: ProjectService = ProjectService::new();
/// The handle `setup` opened: the listing client of this file.
static mut FILES: *mut u8 = core::ptr::null_mut();
/// The handle of core/ui that `setup` opened: null when the image has no core/ui (the warnings are then in the log only).
static mut UI: *mut u8 = core::ptr::null_mut();
/// UI passes since `init`: the clock when the image has no block counter.
static mut PASSES: u32 = 0;
/// The listing in progress: its ticket, and the entry the poll fills.
static mut TICKET: u32 = 0;
#[repr(C, align(4))]
struct Entry([u8; 304]);
static mut ENTRY: Entry = Entry([0; 304]);

/// A NUL-terminated copy of a store path (at most 63 bytes, which `Path` never exceeds).
fn cpath(p: &[u8], out: &mut [u8; 64]) -> *const u8 {
    let n = p.len().min(63);
    let mut i = 0;
    while i < n {
        out[i] = p[i];
        i += 1;
    }
    out[n] = 0;
    out.as_ptr()
}

pub struct DkEnv;

impl Env for DkEnv {
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
                read_volatile(addr_of!(PASSES)).wrapping_mul(8)
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
                read_volatile(addr_of!(FILES)),
                STORE,
                b"projects\0".as_ptr(),
                None,
                core::ptr::null_mut(),
            );
            if t > 0 {
                TICKET = t as u32;
                return 0;
            }
            if t == 0 {
                return dkrt_sdk::E_NOT_FOUND;
            }
            t
        }
    }
    fn list_next(&mut self, row: &mut ListRow) -> ListPoll {
        // SAFETY: `ENTRY` is this file's own buffer of one `dk_file_entry` (304 bytes).
        unsafe {
            loop {
                let r = sys::dk_files_poll(
                    read_volatile(addr_of!(FILES)),
                    TICKET,
                    addr_of_mut!(ENTRY) as *mut u8,
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
                let e = &*addr_of!(ENTRY);
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
                read_volatile(addr_of!(FILES)),
                cpath(path, &mut c),
                buf.as_mut_ptr(),
                buf.len() as u32,
            )
        }
    }
    fn post(&mut self) -> i32 {
        // SAFETY: the step and the done function are this file's; the job reaches the state through the statics.
        unsafe { sys::dk_io_post(Some(io_step), Some(io_done), core::ptr::null_mut()) }
    }
    fn log(&mut self, kind: u16, args: [u32; 5]) {
        // SAFETY: the log of the runtime.
        unsafe { CORE.log(kind, LOG_INFO, args) }
    }
    fn toast(&mut self, reason: u32) {
        // one short line for a file that is not used (docs/design/digikit-project.md section 6); the facts are in the log (PROJECT_IGNORED)
        show(&[if reason == Ignore::Store as u32 {
            b"PROJECT DATA NOT READ"
        } else {
            b"PROJECT DATA IGNORED"
        }]);
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
            ModuleWarning::Missing => show(&[b"MODULE MISSING: ", id.name()]),
            ModuleWarning::NoFit => show_name(id.name(), b" DOES NOT FIT"),
            ModuleWarning::Refused => show_name(id.name(), b" NOT LOADED"),
            ModuleWarning::NotKept => show_name(id.name(), b" NOT KEPT"),
        }
    }
}

/// Characters a toast shows (`DK_UI_TOAST` of <dk/platform/ui.h>, the terminator included).
const TOAST_MAX: usize = 32;

/// A toast of the parts joined, upper case, cut to what a toast shows. Nothing when the image has no core/ui.
fn show(parts: &[&[u8]]) {
    // SAFETY: the handle is set in `setup` and read in the UI task only.
    let ui = unsafe { read_volatile(addr_of!(UI)) };
    if ui.is_null() {
        return;
    }
    let mut text = [0u8; TOAST_MAX];
    let mut n = 0;
    for part in parts {
        for &b in part.iter() {
            if n < TOAST_MAX - 1 {
                text[n] = b.to_ascii_uppercase();
                n += 1;
            }
        }
    }
    // SAFETY: `text` is NUL terminated; the result (0 shown, else a failure value) changes nothing for the service.
    unsafe {
        sys::dk_ui_toast(ui, text.as_ptr());
    }
}

/// `<name><suffix>`, with the name cut so that the suffix stays whole.
fn show_name(name: &[u8], suffix: &[u8]) {
    let keep = name.len().min(TOAST_MAX - 1 - suffix.len());
    show(&[&name[..keep], suffix]);
}

/// The bytes of a NUL-terminated string of at most `max` bytes.
unsafe fn cstr<'a>(p: *const u8, max: usize) -> &'a [u8] {
    let mut n = 0;
    while n < max && *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n)
}

pub struct DkFiles;

impl Files for DkFiles {
    fn create(&mut self, path: &[u8], size: u32) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: the handle was opened in `init`; the call runs in the I/O step.
        unsafe {
            sys::dk_files_create(
                read_volatile(addr_of!(FILES)),
                STORE,
                cpath(path, &mut c),
                size,
                core::ptr::null(),
                REPLACE,
            )
        }
    }
    fn write(&mut self, h: i32, buf: &[u8]) -> i32 {
        // SAFETY: as above; the image is 16-byte aligned.
        unsafe {
            sys::dk_files_write(
                read_volatile(addr_of!(FILES)),
                h,
                0,
                buf.as_ptr(),
                buf.len() as u32,
            )
        }
    }
    fn close(&mut self, h: i32) -> i32 {
        // SAFETY: as above.
        unsafe { sys::dk_files_close(read_volatile(addr_of!(FILES)), h, core::ptr::null_mut()) }
    }
    fn abort(&mut self, h: i32) {
        // SAFETY: as above.
        unsafe {
            sys::dk_files_abort(read_volatile(addr_of!(FILES)), h);
        }
    }
    fn delete(&mut self, path: &[u8]) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: as above.
        unsafe { sys::dk_files_delete(read_volatile(addr_of!(FILES)), STORE, cpath(path, &mut c)) }
    }
}

/// The write job's step, in the I/O task (`dk_io_post`).
unsafe extern "C" fn io_step(_arg: *mut u8) -> i32 {
    (*addr_of_mut!(PROJECT)).io_step(&mut DkFiles)
}

/// The end of the write job, in the UI task.
unsafe extern "C" fn io_done(_arg: *mut u8, status: i32) {
    (*addr_of_mut!(PROJECT)).io_done(status)
}

/// `setup` of the file (the first step of `init`): the listing client that this file's `init` is the only place to open, and the project scope table the runtime's settings overlay forwards to.
pub unsafe fn setup(core: &ImportCore) -> i32 {
    let ctx = dkrt_svc_rt::init_ctx();
    let mut f: *mut u8 = core::ptr::null_mut();
    let r = sys::dk_files_open(ctx, &mut f);
    if r != 0 {
        return r;
    }
    *addr_of_mut!(FILES) = f;
    // optional: without core/ui the result is a failure and the handle stays null
    let mut ui: *mut u8 = core::ptr::null_mut();
    if sys::dk_ui_open(ctx, &mut ui) == 0 {
        *addr_of_mut!(UI) = ui;
    }
    core.settings_overlay_set(&SCOPE);
    0
}

/// The tick of the file (the UI task, once per pass): what the hooks recorded, the work in progress, the next piece.
pub unsafe extern "C" fn tick(_ctx: *mut u8) {
    *addr_of_mut!(PASSES) = read_volatile(addr_of!(PASSES)).wrapping_add(1);
    (*addr_of_mut!(PROJECT)).tick(&mut DkEnv, &*addr_of!(DKRT_PROJECT_PENDING));
}

unsafe fn bytes<'a>(p: *const u8, n: u32) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(p, n as usize)
    }
}

unsafe extern "C" fn scope_get(
    id: *const u8,
    id_len: u32,
    key: *const u8,
    key_len: u32,
    out: *mut u8,
    cap: u32,
) -> i32 {
    let o: &mut [u8] = if cap == 0 {
        &mut []
    } else {
        core::slice::from_raw_parts_mut(out, cap as usize)
    };
    (*addr_of!(PROJECT)).scope_get(bytes(id, id_len), bytes(key, key_len), o)
}

unsafe extern "C" fn scope_put(
    id: *const u8,
    id_len: u32,
    key: *const u8,
    key_len: u32,
    value: *const u8,
    len: u32,
) -> i32 {
    (*addr_of_mut!(PROJECT)).scope_put(bytes(id, id_len), bytes(key, key_len), bytes(value, len))
}

unsafe extern "C" fn scope_remove(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32 {
    (*addr_of_mut!(PROJECT)).scope_remove(bytes(id, id_len), bytes(key, key_len))
}

/// The table `dkc_settings_overlay_set` hands to the runtime.
pub static SCOPE: SettingsOverlay = SettingsOverlay {
    version: SETTINGS_OVERLAY_VERSION,
    get: scope_get,
    put: scope_put,
    remove: scope_remove,
};

/// `dk_project_current` (export of the file): the slot and the name of the current project, `E_NOT_FOUND` without one.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_current(slot: *mut u32, name: *mut u8) -> i32 {
    match (*addr_of!(PROJECT)).current() {
        Ok((s, n)) => {
            if !slot.is_null() {
                *slot = s;
            }
            if !name.is_null() {
                let mut i = 0;
                while i < 16 {
                    *name.add(i) = n[i];
                    i += 1;
                }
            }
            0
        }
        Err(e) => e,
    }
}

/// `dk_project_status` (export of the file): the state bits.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_status() -> u32 {
    (*addr_of!(PROJECT)).status()
}
