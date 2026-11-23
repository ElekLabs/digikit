use super::AddressMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::mem::{call_at, read32, write32};

extern "C" fn twice(a: usize, _b: usize, _c: usize, _d: usize, _e: usize, _f: usize, _g: usize, _h: usize, _i: usize, _j: usize, _k: usize, _l: usize) -> usize {
    a * 2
}

#[test]
fn word_access_goes_through_the_mapping() {
    let mut ram = [0u32; 4];
    let map = AddressMap::new();
    map.map(0x2000_0000..0x2000_0010, ram.as_mut_ptr() as *mut u8);
    map.with_addresses(|| {
        unsafe {
            write32(DevAddr::new(0x2000_0008), 0xdead_beef);
            assert_eq!(read32(DevAddr::new(0x2000_0008)), 0xdead_beef);
        }
        assert_eq!(ram[2], 0xdead_beef);
    });
}

#[test]
fn a_host_pointer_maps_back_to_its_device_address() {
    let mut ram = [0u8; 16];
    let map = AddressMap::new();
    map.map(0x1000..0x1010, ram.as_mut_ptr());
    map.with_addresses(|| {
        let a = DevAddr::of_ptr(&ram[5] as *const u8);
        assert_eq!(a.get(), 0x1005);
        assert_eq!(a.ptr::<u8>(), &mut ram[5] as *mut u8);
    });
}

#[test]
fn a_function_has_one_device_address_and_is_called_through_it() {
    let map = AddressMap::new();
    map.with_addresses(|| {
        let a = DevAddr::of_fn(twice as usize);
        assert_eq!(a, DevAddr::of_fn(twice as usize));
        assert!(!a.is_null());
        assert_eq!(unsafe { call_at(a, &[21]) }, 42);
        assert_eq!(unsafe { call_at(DevAddr::NULL, &[21]) }, 0);
    });
}

#[test]
#[should_panic(expected = "0x30000000 is not mapped")]
fn an_unmapped_address_panics_with_the_address() {
    let map = AddressMap::new();
    map.with_addresses(|| {
        unsafe { read32(DevAddr::new(0x3000_0000)) };
    });
}

#[test]
#[should_panic(expected = "overlaps")]
fn overlapping_ranges_are_refused() {
    let mut a = [0u8; 8];
    let map = AddressMap::new();
    map.map(0x100..0x108, a.as_mut_ptr());
    map.map(0x104..0x10c, a.as_mut_ptr());
}

#[test]
#[should_panic(expected = "is in no mapped range")]
fn an_unmapped_host_pointer_panics_with_the_pointer() {
    let ram = [0u8; 4];
    let other = [0u8; 4];
    let map = AddressMap::new();
    map.map(0x1000..0x1004, ram.as_ptr() as *mut u8);
    map.with_addresses(|| {
        DevAddr::of_ptr(other.as_ptr());
    });
}

#[test]
#[should_panic(expected = "overlaps the function range")]
fn a_range_in_the_function_range_is_refused() {
    let mut a = [0u8; 8];
    AddressMap::new().map(0xefff_fffc..0xf000_0004, a.as_mut_ptr());
}

#[test]
fn scopes_nest_and_restore() {
    let mut a = [0u8; 4];
    let mut b = [0u8; 4];
    let one = AddressMap::new();
    one.map(0x10..0x14, a.as_mut_ptr());
    let two = AddressMap::new();
    two.map(0x20..0x24, b.as_mut_ptr());
    one.with_addresses(|| {
        two.with_addresses(|| assert_eq!(DevAddr::of_ptr(b.as_ptr()).get(), 0x20));
        assert_eq!(DevAddr::of_ptr(a.as_ptr()).get(), 0x10);
    });
}

#[test]
fn an_unwind_restores_the_previous_space() {
    let mut a = [0u8; 4];
    let one = AddressMap::new();
    one.map(0x10..0x14, a.as_mut_ptr());
    let two = AddressMap::new();
    one.with_addresses(|| {
        let r = catch_unwind(AssertUnwindSafe(|| two.with_addresses(|| panic!("body"))));
        assert!(r.is_err());
        assert_eq!(DevAddr::of_ptr(a.as_ptr()).get(), 0x10);
    });
}
