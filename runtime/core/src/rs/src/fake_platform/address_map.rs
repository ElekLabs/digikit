//! `AddressMap`: the host's map between device addresses and host memory and code, the `AddressSpace` that `FakePlatform` installs for `DevAddr`.
//!
//! `map(range, host_ptr)` says that the device addresses of `range` are the bytes at `host_ptr`; `intern_fn` gives a host function a device address in a code range that no memory
//! mapping covers. An address that nothing covers is an error of the test, not of the code under test: every lookup answers `None` and `DevAddr` panics with the address.

use core::cell::RefCell;
use core::ops::Range;
use dkrt_abi::dev_addr::{with_space, AddressSpace};
use std::vec::Vec;

/// The first device address that `intern_fn` hands out; one function takes `FN_STEP` addresses.
const FN_BASE: u32 = 0xF000_0000;
const FN_STEP: u32 = 4;

struct Mapping {
    range: Range<u32>,
    host: *mut u8,
}

#[derive(Default)]
pub struct AddressMap {
    maps: RefCell<Vec<Mapping>>,
    fns: RefCell<Vec<usize>>,
}

impl AddressMap {
    pub fn new() -> AddressMap {
        AddressMap::default()
    }

    /// The device addresses of `range` are the bytes from `host`, which stay valid while the map is installed.
    pub fn map(&self, range: Range<u32>, host: *mut u8) {
        assert!(range.start < range.end, "an empty device range");
        assert!(range.end <= FN_BASE, "device range {:#x}..{:#x} overlaps the function range from {FN_BASE:#x}", range.start, range.end);
        let mut maps = self.maps.borrow_mut();
        assert!(
            maps.iter().all(|m| range.end <= m.range.start || m.range.end <= range.start),
            "device range {:#x}..{:#x} overlaps a mapped one",
            range.start,
            range.end
        );
        maps.push(Mapping { range, host });
    }

    /// The device address of the host function at `f`: the same address for the same function.
    pub fn intern_fn(&self, f: usize) -> u32 {
        let mut fns = self.fns.borrow_mut();
        let i = match fns.iter().position(|&g| g == f) {
            Some(i) => i,
            None => {
                fns.push(f);
                fns.len() - 1
            }
        };
        FN_BASE + i as u32 * FN_STEP
    }

    /// Run `body` with this map as the address space of `DevAddr` on this thread; the previous space returns afterwards, also on unwind.
    pub fn with_addresses<R>(&self, body: impl FnOnce() -> R) -> R {
        with_space(self, body)
    }
}

impl AddressSpace for AddressMap {
    fn of_host(&self, p: *const u8) -> Option<u32> {
        let p = p as usize;
        for m in self.maps.borrow().iter() {
            let base = m.host as usize;
            let len = (m.range.end - m.range.start) as usize;
            if p >= base && p < base + len {
                return Some(m.range.start + (p - base) as u32);
            }
        }
        None
    }

    fn of_host_fn(&self, f: usize) -> u32 {
        self.intern_fn(f)
    }

    fn to_host(&self, a: u32) -> Option<*mut u8> {
        let maps = self.maps.borrow();
        let m = maps.iter().find(|m| m.range.contains(&a))?;
        Some(m.host.wrapping_add((a - m.range.start) as usize))
    }

    fn to_host_fn(&self, a: u32) -> Option<usize> {
        let i = a.checked_sub(FN_BASE)?;
        if i % FN_STEP != 0 {
            return None;
        }
        self.fns.borrow().get((i / FN_STEP) as usize).copied()
    }
}

#[cfg(test)]
mod tests;
