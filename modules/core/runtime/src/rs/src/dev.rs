//! The device object `dkrt_dev` (struct `DkrtDevice` of `abi.toml`): the facts that differ between devices, as data. The runtime is one image
//! for all devices and has no device value in its code; `patchkit gen` writes the object from the device files (the `gas-dkdv` kind of
//! module.toml) and patchkit places it in `cf.expansion` with the runtime. Services read their own facts from the same object.
use core::ptr::addr_of;
use dkrt_sdk::{DkrtDevice, DEV_HAS_DSP, DEV_HAS_SAFE_KEY, DEV_HAS_SDHC, DEV_HAS_STORE, DEV_MAGIC, MACHINE_ROWS};

extern "C" {
    static dkrt_dev: DkrtDevice;
}

fn d() -> &'static DkrtDevice {
    unsafe { &*addr_of!(dkrt_dev) }
}

/// Does the object carry the capability bit `cap` (`DEV_HAS_*`)?
pub fn has(cap: u32) -> bool {
    d().caps & cap != 0
}

pub fn has_dsp() -> bool {
    has(DEV_HAS_DSP)
}

pub fn has_store() -> bool {
    has(DEV_HAS_STORE)
}

pub fn has_sdhc() -> bool {
    has(DEV_HAS_SDHC)
}

/// The firmware key index held at power-on for safe mode, `None` when the device has no such key.
pub fn safe_key() -> Option<u32> {
    has(DEV_HAS_SAFE_KEY).then(|| d().safe_key)
}

/// Is `a` an address of the cached DDR window?
pub fn ddr_contains(a: u32) -> bool {
    let (base, bytes) = (d().ddr_cached_base, d().ddr_bytes);
    a >= base && a - base < bytes
}

/// The cached DDR window: its base and size.
pub fn ddr_base() -> u32 {
    d().ddr_cached_base
}

pub fn ddr_bytes() -> u32 {
    d().ddr_bytes
}

/// The RAM a soft reset is assumed to keep: its base and size.
pub fn keep_base() -> u32 {
    d().keep_base
}

pub fn keep_bytes() -> u32 {
    d().keep_bytes
}

/// The eSDHC controller of the card (`has_sdhc`).
pub fn esdhc_base() -> u32 {
    d().esdhc_base
}

/// DSP cycles in one block (0 without a DSP).
pub fn dsp_block_cycles() -> u32 {
    d().dsp_block_cycles
}

/// The run-time machine types: the first number and the count (0 without a DSP).
pub fn dyn_type_first() -> u32 {
    d().dyn_type_first
}

pub fn dyn_types() -> u32 {
    d().dyn_types
}

/// Is `t` one of the run-time machine types?
pub fn is_dyn_type(t: u32) -> bool {
    t >= d().dyn_type_first && t - d().dyn_type_first < d().dyn_types
}

/// Is the object one this runtime reads? Its magic, a size that has every field the runtime knows, and a number of machine types that fits the
/// runtime's tables (`MACHINE_ROWS`). Checked once at start.
pub fn valid() -> bool {
    let o = d();
    o.magic == DEV_MAGIC
        && o.size as usize >= core::mem::size_of::<DkrtDevice>()
        && o.dyn_types as usize <= MACHINE_ROWS
        && (o.caps & DEV_HAS_DSP != 0 || o.dyn_types == 0)
}
