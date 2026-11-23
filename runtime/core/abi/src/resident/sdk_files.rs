//! `SdkFilesFns`: the writer front end of the SDK's file calls (`storage/sdk_files.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SdkFilesFns {
    pub create: unsafe extern "C" fn(
        root: u32,
        path: *const u8,
        size: u32,
        hash4: *const u8,
        flags: u32,
    ) -> i32,
    pub write: unsafe extern "C" fn(h: i32, offset: u32, buf: *const u8, len: u32) -> i32,
    pub close: unsafe extern "C" fn(h: i32, out: *mut u8) -> i32,
    pub abort: unsafe extern "C" fn(h: i32) -> i32,
    pub delete: unsafe extern "C" fn(root: u32, path: *const u8) -> i32,
    pub commit: unsafe extern "C" fn() -> i32,
    pub pending: unsafe extern "C" fn() -> i32,
}
