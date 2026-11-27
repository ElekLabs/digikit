//! The package's entries of the host directory, through the real entry (`attach` over the real resident table).
use super::*;
use crate::fake_platform::address_map::AddressMap;
use crate::test_lock::LOCK;
use core::ptr::addr_of_mut;
use dkrt_abi::host_dir::HostDir;
use dkrt_sdk::{HOST_CORE_OWNED, HOST_COUNT, HOST_PACKAGE_OWNED};

fn directory() -> HostDir {
    unsafe { *addr_of!(crate::host_dir::DKRT_HOST_DIR) }
}

/// The entry of the package fills every id with `owner = "package"` (walking the generated list, so an id appended to abi.toml is covered), each with the address of its own
/// static, and none of the core's. The package never marks the directory ready: that is the core's last step.
#[test]
fn the_package_entry_publishes_every_package_owned_id_with_the_address_of_its_static() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let map = AddressMap::new();
        let at = |p: *mut u8, len: usize| map.place(p, len);
        let want = [
            (HOST_DSP, at(addr_of_mut!(DKRT_DSP) as *mut u8, core::mem::size_of_val(&DKRT_DSP))),
            (HOST_PROVIDERS, at(addr_of_mut!(DKRT_PROVIDERS) as *mut u8, core::mem::size_of_val(&DKRT_PROVIDERS))),
            (HOST_TICKS, at(addr_of_mut!(DKRT_TICKS) as *mut u8, core::mem::size_of_val(&DKRT_TICKS))),
            (HOST_TABLE_BAD, at(addr_of_mut!(DKRT_TABLE_BAD), 1)),
        ];
        let _space = map.install();
        crate::host_dir::clear();
        assert!((0..HOST_COUNT as u32).all(|id| directory().get(id).is_none()), "empty before the entry");
        crate::package::attach(&crate::resident::TABLE).expect("the package accepts the table of this build");
        let dir = directory();
        for &id in HOST_PACKAGE_OWNED.iter() {
            assert!(dir.get(id).is_some(), "package-owned id {id} is filled at attach (a new id needs a row in `address_of`)");
        }
        for (id, addr) in want {
            assert_eq!(dir.get(id), Some(addr), "id {id}");
        }
        for &id in HOST_CORE_OWNED.iter() {
            assert!(dir.get(id).is_none(), "core-owned id {id} is not the package's to fill");
        }
        assert!(!dir.is_ready(), "the core marks the directory ready, not the package");
        crate::host_dir::clear();
    }
}
