//! The package's entries of the host directory, through the real entry (`attach` over the real resident table).
use super::*;
use crate::fake_platform::address_map::AddressMap;
use crate::test_lock::LOCK;
use core::ptr::addr_of_mut;
use dkrt_abi::host_dir::HostDir;
use dkrt_sdk::HOST_COUNT;

fn directory() -> HostDir {
    unsafe { *addr_of!(crate::host_dir::DKRT_HOST_DIR) }
}

/// The entry of the package fills every id of the directory, each with the address of its own static.
#[test]
fn the_package_entry_publishes_every_id_with_the_address_of_its_static() {
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
        assert_eq!(want.len(), OWN, "a new id of abi.toml before the core's needs its row here and in `addresses`");
        let _space = map.install();
        crate::host_dir::clear();
        assert!((0..dkrt_sdk::HOST_COUNT as u32).all(|id| directory().get(id).is_none()), "empty before the entry");
        crate::package::attach(&crate::resident::TABLE).expect("the package accepts the table of this build");
        let dir = directory();
        for (id, addr) in want {
            assert_eq!(dir.get(id), Some(addr), "id {id}");
        }
        assert!((0..OWN as u32).all(|id| dir.get(id).is_some()), "every id of the package is filled");
        assert!((OWN as u32..HOST_COUNT as u32).all(|id| dir.get(id).is_none()), "the core's ids are not the package's to fill");
        crate::host_dir::clear();
    }
}
