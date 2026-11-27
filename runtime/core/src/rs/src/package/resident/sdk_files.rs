//! The writer front end of the SDK's file calls.
use super::table;

#[inline(always)]
pub(crate) unsafe fn sdk_create(root: u32, path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    (table().sdk_files.create)(root, path, size, hash4, flags)
}

#[inline(always)]
pub(crate) unsafe fn sdk_write(h: i32, offset: u32, buf: *const u8, len: u32) -> i32 {
    (table().sdk_files.write)(h, offset, buf, len)
}

#[inline(always)]
pub(crate) unsafe fn sdk_close(h: i32, out: *mut u8) -> i32 {
    (table().sdk_files.close)(h, out)
}

#[inline(always)]
pub(crate) unsafe fn sdk_abort(h: i32) -> i32 {
    (table().sdk_files.abort)(h)
}

#[inline(always)]
pub(crate) unsafe fn sdk_delete(root: u32, path: *const u8) -> i32 {
    (table().sdk_files.delete)(root, path)
}

#[inline(always)]
pub(crate) unsafe fn sdk_commit() -> i32 {
    (table().sdk_files.commit)()
}

#[inline(always)]
pub(crate) unsafe fn sdk_pending() -> i32 {
    (table().sdk_files.pending)()
}
