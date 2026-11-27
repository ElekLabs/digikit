//! The host directory: where the package tells the resident core the addresses of its statics.
use super::table;

/// Set the directory entry of `id` to the device address `addr`; false for an id the core does not know.
#[inline(always)]
pub(crate) unsafe fn publish(id: u32, addr: u32) -> bool {
    (table().host_dir.publish)(id, addr)
}
