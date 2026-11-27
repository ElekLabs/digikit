//! `HostDirFns`: the host directory (`DKRT_HOST_DIR`, `host_dir.rs` of the runtime crate): where the package tells the resident core the addresses of its statics.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostDirFns {
    /// Set the directory entry of `id` (`HOST_*` of `abi.toml`) to the device address `addr`: true, or false for an id this core does not know.
    pub publish: unsafe extern "C" fn(id: u32, addr: u32) -> bool,
}
