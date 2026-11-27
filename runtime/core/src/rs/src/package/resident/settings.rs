//! The settings broker: the runtime's own keys and one client per module.
use super::table;

#[inline(always)]
pub(crate) unsafe fn get(key: &[u8], out: &mut [u8]) -> i32 {
    (table().settings.get)(key.as_ptr(), key.len() as u32, out.as_mut_ptr(), out.len() as u32)
}

#[inline(always)]
pub(crate) unsafe fn put(key: &[u8], value: &[u8]) -> i32 {
    (table().settings.put)(key.as_ptr(), key.len() as u32, value.as_ptr(), value.len() as u32)
}

#[inline(always)]
pub(crate) unsafe fn module_client() -> i32 {
    (table().settings.module_client)()
}

#[inline(always)]
pub(crate) unsafe fn module_get(client: u32, key: *const u8, out: *mut u8, cap: u32) -> i32 {
    (table().settings.module_get)(client, key, out, cap)
}

#[inline(always)]
pub(crate) unsafe fn module_put(client: u32, key: *const u8, value: *const u8, len: u32) -> i32 {
    (table().settings.module_put)(client, key, value, len)
}

#[inline(always)]
pub(crate) unsafe fn module_remove(client: u32, key: *const u8) -> i32 {
    (table().settings.module_remove)(client, key)
}

/// The record's area and what it uses now.
#[inline(always)]
pub(crate) unsafe fn capacity() -> (u32, u32) {
    let mut out = [0u32; 2];
    (table().settings.capacity)(out.as_mut_ptr());
    (out[0], out[1])
}
