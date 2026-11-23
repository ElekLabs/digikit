//! The device object `dkrt_dev` (struct `DkrtDevice` of `abi.toml`): the facts that differ between devices, as data. The runtime is one image
//! for all devices and has no device value in its code; `patchkit gen` writes the object from the device files (the `gas-dkdv` kind of
//! module.toml) and patchkit places it in `cf.expansion` with the runtime. This module is the one reader of the object: `RealPlatform` (`platform.rs`) fills
//! its `DeviceProfile` from it, and the rest of the runtime asks the profile (`crate::platform::profile()`, `Platform::has_dsp` and the other facts); a service takes
//! its profile from the generated table by device id. Host tests run `RealPlatform` as a device profile (`platform::with_profile`), so only `valid` and the
//! tests of this file read the object.
#![cfg_attr(test, allow(dead_code))]
use core::ptr::addr_of;
use core::sync::atomic::{AtomicU8, Ordering};
use dkrt_sdk::{DkrtDevice, DEV_HAS_DSP, DEV_HAS_SAFE_KEY, DEV_HAS_SDHC, DEV_HAS_STORE, DEV_MAGIC, DEV_VERSION, MACHINE_ROWS};

extern "C" {
    static dkrt_dev: DkrtDevice;
}

fn d() -> &'static DkrtDevice {
    unsafe { &*addr_of!(dkrt_dev) }
}

/// The capability bits of the object.
fn caps() -> u32 {
    d().caps
}

/// Does the device have the capability `cap` (`DEV_HAS_*`)? An object this runtime cannot read (`valid`) has none.
pub fn has(cap: u32) -> bool {
    valid() && caps() & cap != 0
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

/// The tracks of the device (0: the device file gives none).
pub fn tracks() -> u32 {
    if valid() {
        d().tracks
    } else {
        0
    }
}

/// The panel code of each logical key, as the profile carries them (`patchkit gen` writes 1 to 255, or 0 for a key the device lacks). An object this runtime cannot read has none.
pub fn key_codes() -> [u8; dkrt_abi::platform::KEY_SLOTS] {
    if valid() {
        core::array::from_fn(|i| d().key_codes[i] as u8)
    } else {
        [0; dkrt_abi::platform::KEY_SLOTS]
    }
}

/// The bits of a key event's flags word (`[input.key_flags]`); an object this runtime cannot read gets the usual bits.
pub fn key_flags() -> dkrt_abi::platform::KeyFlagBits {
    use dkrt_abi::platform::KeyFlagBits;
    if valid() {
        KeyFlagBits {
            press: d().key_flag_press as u8,
            func: d().key_flag_func as u8,
            repeat: d().key_flag_repeat as u8,
            release: d().key_flag_release as u8,
        }
    } else {
        KeyFlagBits { press: 0, func: 1, repeat: 3, release: 4 }
    }
}

/// Is `t` one of the run-time machine types?
pub fn is_dyn_type(t: u32) -> bool {
    t >= d().dyn_type_first && t - d().dyn_type_first < d().dyn_types
}

/// Validity of the object: 0 not checked yet, 1 valid, 2 invalid. The object is constant, so one check answers for the whole run.
static CHECKED: AtomicU8 = AtomicU8::new(0);

/// Is the object one this runtime reads? Its magic, its version, a size that has every field the runtime knows, and a number of machine types
/// that is the runtime's table size (`MACHINE_ROWS`) or 0 (a device without a DSP has none and no machine rows are used). Checked once, on the
/// first call (`init_once` makes it); the answer is kept.
pub fn valid() -> bool {
    match CHECKED.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let ok = check(d());
            CHECKED.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            ok
        }
    }
}

/// The rules of `valid` for one object.
fn check(o: &DkrtDevice) -> bool {
    o.magic == DEV_MAGIC
        && o.version == DEV_VERSION
        && o.size as usize >= core::mem::size_of::<DkrtDevice>()
        && (o.dyn_types as usize == MACHINE_ROWS || o.dyn_types == 0)
        && (o.caps & DEV_HAS_DSP != 0 || o.dyn_types == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::with_profile;
    use dkrt_abi::platform::{DeviceProfile, Platform};
    use dkrt_sdk::*;

    #[test]
    fn the_host_fixtures_object_is_valid_and_has_every_capability() {
        assert!(valid());
        assert!(has_dsp() && has_store() && has_sdhc() && safe_key() == Some(19));
        assert!(is_dyn_type(16) && is_dyn_type(23) && !is_dyn_type(15) && !is_dyn_type(24));
        // the fixture's object and the profile the host tests run as are one device
        let p = dkrt_abi::profiles::DT2;
        assert_eq!((p.ddr_base, p.ddr_bytes, p.keep_base, p.dyn_type_first, p.dyn_types), (ddr_base(), ddr_bytes(), keep_base(), dyn_type_first(), dyn_types()));
        assert_eq!((tracks(), key_codes()), (16, [0; dkrt_abi::platform::KEY_SLOTS]));
    }

    #[test]
    fn an_object_the_runtime_cannot_read_is_not_valid() {
        let good = unsafe { core::ptr::read(addr_of!(dkrt_dev)) };
        assert!(check(&good));
        let bad = |f: &dyn Fn(&mut DkrtDevice)| {
            let mut o = unsafe { core::ptr::read(addr_of!(dkrt_dev)) };
            f(&mut o);
            check(&o)
        };
        assert!(!bad(&|o| o.magic ^= 1));
        assert!(!bad(&|o| o.version += 1));
        assert!(!bad(&|o| o.size -= 4));
        // the machine rows are a table of MACHINE_ROWS: a count of 0 (no DSP) or exactly that
        assert!(!bad(&|o| o.dyn_types = MACHINE_ROWS as u32 - 1));
        assert!(!bad(&|o| o.dyn_types = MACHINE_ROWS as u32 + 1));
        assert!(bad(&|o| {
            o.dyn_types = 0;
            o.caps &= !DEV_HAS_DSP;
        }));
        // machine types without a DSP are a contradiction
        assert!(!bad(&|o| {
            o.caps &= !DEV_HAS_DSP;
        }));
    }

    /// The first device file's profile with some facts changed: a device that lacks a capability.
    fn device(f: impl FnOnce(&mut DeviceProfile)) -> DeviceProfile {
        let mut p = dkrt_abi::profiles::DT2;
        f(&mut p);
        p
    }

    #[test]
    fn without_the_safe_key_capability_no_key_starts_safe_mode() {
        with_profile(device(|p| p.safe_key = None), || assert_eq!(crate::platform::PLATFORM.safe_key(), None));
        assert_eq!(crate::platform::PLATFORM.safe_key(), Some(19));
    }

    #[test]
    fn without_a_dsp_there_is_no_dsp_area_no_machine_rows_and_no_block_clock() {
        // the health report walks the arena of the runtime state, which a test of the test lock may have up
        let _g = crate::fake_platform::test_arena::TestArena::lock();
        // (the fixture has no store area, so its health runs without the store: `store_total` would be the area's size)
        with_profile(device(|p| {
            p.has_dsp = false;
            p.has_store = false;
            p.dyn_types = 0;
            p.dsp_block_cycles = 0;
        }), || unsafe {
            assert_eq!(crate::package::dsp::area_bytes(), 0);
            assert_eq!(crate::health::dkrt_blocks(), 0);
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::health::dkrt_health_light(&mut h, false);
            assert!(h.flags & HEALTH_NO_DSP != 0 && h.flags & HEALTH_DSP == 0);
            assert_eq!((h.dsp_area_total, h.dsp_area_free, h.dsp_area_largest), (0, 0, 0));
        });
        // with a DSP the fixture's area is there and the flag is clear
        with_profile(device(|p| p.has_store = false), || unsafe {
            assert_ne!(crate::package::dsp::area_bytes(), 0);
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::health::dkrt_health_light(&mut h, false);
            assert!(h.flags & HEALTH_NO_DSP == 0);
        });
    }

    #[test]
    fn without_a_store_every_store_call_answers_not_found_and_nothing_is_written() {
        // the health report walks the arena of the runtime state, which a test of the test lock may have up
        let _g = crate::fake_platform::test_arena::TestArena::lock();
        with_profile(device(|p| p.has_store = false), || unsafe {
            assert_eq!(crate::card::state(), CARD_TOO_SMALL);
            assert!(matches!(crate::store::open(), Err(E_NOT_FOUND)));
            assert_eq!(crate::store::read_sector(0, core::ptr::null_mut()), E_NOT_FOUND);
            // a card write is refused: the card never counts as ours
            assert!(!crate::card::writable());
            let mut h: DkrtHealth = core::mem::zeroed();
            crate::health::dkrt_health_light(&mut h, false);
            assert_eq!(h.store_total, 0);
        });
    }

    #[test]
    fn without_an_sdhc_controller_the_card_is_never_busy() {
        with_profile(device(|p| p.has_sdhc = false), || assert!(!unsafe { crate::api::card_busy() }));
    }
}
