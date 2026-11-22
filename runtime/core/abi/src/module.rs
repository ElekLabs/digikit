//! The module record and the names the runtime and the system package both use: `Module` (one slot of the arena's module table), `Ctx` (what a module's
//! `dk_module_ctx *` points to), the width of the names the log shows and the provider index.
use dkrt_sdk::*;

/// One module record: the used flag, the name, the version and the arena block of its code and data. The package reads the records in `PackageState`: the layout is
/// part of the major version of the resident table.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Module {
    pub used: u32,
    pub name: [u8; 32],
    pub version: u32,
    pub base: u32,
    pub size: u32,
    pub fini: u32,
}

/// "DKCX": the first word of a live context.
pub const CTX_MAGIC: u32 = 0x444b_4358;

/// What a `dk_module_ctx *` points to: the record of the module that `init`, `fini`, `tick` and `menu` belong to.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ctx {
    pub magic: u32,
    pub rec: u32,
}

/// Bytes of a module's name in the log events (the name part of its id).
pub const LOG_NAME: usize = 12;

/// The index a provider's `MOD_LOAD` event carries in place of a selection index.
pub const PROVIDER_IDX: u32 = 0xffff_fffe;

/// The name a module event shows (`LOG_NAME` bytes): the name part of the module id when the loader read the header (`name`, not empty), else the file leaf without its `.dkm`.
#[inline]
pub fn shown_name(leaf: &[u8], name: &[u8; LOG_NAME]) -> [u8; LOG_NAME] {
    if name[0] != 0 {
        return *name;
    }
    let leaf = leaf.strip_suffix(b".dkm").unwrap_or(leaf);
    let mut out = [0u8; LOG_NAME];
    let n = leaf.len().min(LOG_NAME);
    out[..n].copy_from_slice(&leaf[..n]);
    out
}

#[cfg(test)]
mod tests;
