//! `SdkFilesFns`: the writer front end of the SDK's file calls.
use crate::storage::sdk_files as sdk;
use dkrt_abi::resident::SdkFilesFns;

unsafe extern "C" fn create(root: u32, path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    sdk::sdk_create(root, path, size, hash4, flags)
}
unsafe extern "C" fn write(h: i32, offset: u32, buf: *const u8, len: u32) -> i32 {
    sdk::sdk_write(h, offset, buf, len)
}
unsafe extern "C" fn close(h: i32, out: *mut u8) -> i32 {
    sdk::sdk_close(h, out)
}
unsafe extern "C" fn abort(h: i32) -> i32 {
    sdk::sdk_abort(h)
}
unsafe extern "C" fn delete(root: u32, path: *const u8) -> i32 {
    sdk::sdk_delete(root, path)
}
unsafe extern "C" fn commit() -> i32 {
    sdk::sdk_commit()
}
unsafe extern "C" fn pending() -> i32 {
    sdk::sdk_pending()
}

pub(super) static FNS: SdkFilesFns = SdkFilesFns { create, write, close, abort, delete, commit, pending };
