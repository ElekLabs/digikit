//! The SDK imports the engine runs over (files, I/O job, block clock, toast) and the consts of their answers. A host build (the assembly test of the runtime runs the declarations of `service!`) has no SDK imports to link: the calls are stubs with the answer of a missing service (but `dk_files_open`, which opens a dummy handle).

/// `DK_FILES_STORE` and `DK_FILE_REPLACE` (<dk/platform/files.h>).
pub(super) const STORE: u32 = 2;
pub(super) const REPLACE: u32 = 1;
pub(super) const FILE_KIND: u32 = 1;
/// The listing rows of `dk_files_poll`: DK_FILES_PENDING 0, ROW 1, END 2.
pub(super) const POLL_ROW: i32 = 1;
pub(super) const POLL_END: i32 = 2;
const E_MISSING: i32 = -73;

#[cfg(target_arch = "m68k")]
mod real {
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
mod stub {
    pub type IoStepFn = unsafe extern "C" fn(*mut u8) -> i32;
    pub type IoDoneFn = unsafe extern "C" fn(*mut u8, i32);
    const MISSING: i32 = super::E_MISSING;
    /// The listing client opens: the handle is a dummy that the other stubs ignore, so a host test can start the owner.
    pub unsafe fn dk_files_open(_: *mut u8, out: *mut *mut u8) -> i32 {
        *out = 1 as *mut u8;
        0
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

#[cfg(target_arch = "m68k")]
pub(super) use real::*;
#[cfg(not(target_arch = "m68k"))]
pub(super) use stub::*;
