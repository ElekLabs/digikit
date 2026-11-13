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

#[cfg(test)]
thread_local! {
    /// The capability bits a host test runs with (`with_caps`); `None`: the object's.
    static CAPS: core::cell::Cell<Option<u32>> = const { core::cell::Cell::new(None) };
}

/// The capability bits: the object's, and in a host test the ones `with_caps` set.
fn caps() -> u32 {
    #[cfg(test)]
    if let Some(c) = CAPS.with(|c| c.get()) {
        return c;
    }
    d().caps
}

/// Run `f` as a device with exactly the capability bits `caps` (host tests of the paths a missing capability takes).
#[cfg(test)]
pub fn with_caps<R>(caps: u32, f: impl FnOnce() -> R) -> R {
    CAPS.with(|c| c.set(Some(caps)));
    let r = f();
    CAPS.with(|c| c.set(None));
    r
}

/// Does the object carry the capability bit `cap` (`DEV_HAS_*`)?
pub fn has(cap: u32) -> bool {
    caps() & cap != 0
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

#[cfg(test)]
mod tests {
    use super::*;
    use dkrt_sdk::*;

    const ALL: u32 = DEV_HAS_DSP | DEV_HAS_STORE | DEV_HAS_SDHC | DEV_HAS_SAFE_KEY;

    #[test]
    fn the_host_fixtures_object_is_valid_and_has_every_capability() {
        assert!(valid());
        assert!(has_dsp() && has_store() && has_sdhc() && safe_key() == Some(19));
        assert!(is_dyn_type(16) && is_dyn_type(23) && !is_dyn_type(15) && !is_dyn_type(24));
    }

    #[test]
    fn without_the_safe_key_capability_no_key_starts_safe_mode() {
        with_caps(ALL & !DEV_HAS_SAFE_KEY, || assert_eq!(safe_key(), None));
    }

    #[test]
    fn without_a_dsp_there_is_no_dsp_area_no_machine_rows_and_no_block_clock() {
        // (the fixture has no store area, so its health runs without the store: `store_total` would be the area's size)
        with_caps(ALL & !DEV_HAS_DSP & !DEV_HAS_STORE, || unsafe {
            assert_eq!(crate::dsp::area_bytes(), 0);
            assert_eq!(crate::dkrt_blocks(), 0);
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::dkrt_health_light(&mut h, false);
            assert!(h.flags & HEALTH_NO_DSP != 0 && h.flags & HEALTH_DSP == 0);
            assert_eq!((h.dsp_area_total, h.dsp_area_free, h.dsp_area_largest), (0, 0, 0));
        });
        // with a DSP the fixture's area is there and the flag is clear
        with_caps(ALL & !DEV_HAS_STORE, || unsafe {
            assert_ne!(crate::dsp::area_bytes(), 0);
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::dkrt_health_light(&mut h, false);
            assert!(h.flags & HEALTH_NO_DSP == 0);
        });
    }

    #[test]
    fn without_a_store_every_store_call_answers_not_found_and_nothing_is_written() {
        with_caps(ALL & !DEV_HAS_STORE, || unsafe {
            assert_eq!(crate::card::state(), CARD_TOO_SMALL);
            assert!(matches!(crate::store::open(), Err(E_NOT_FOUND)));
            assert_eq!(crate::store::read_sector(0, core::ptr::null_mut()), E_NOT_FOUND);
            // a card write is refused: the card never counts as ours
            assert!(!crate::card::writable());
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::dkrt_health_light(&mut h, false);
            assert_eq!(h.store_total, 0);
        });
    }

    #[test]
    fn without_an_sdhc_controller_the_card_is_never_busy() {
        with_caps(ALL & !DEV_HAS_SDHC, || assert!(!unsafe { crate::card_busy() }));
    }
}
