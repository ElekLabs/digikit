//! `DevAddr`: an address in the device's 32-bit space (RAM, code, MMIO, firmware tables) as its own type, so that a host pointer cannot pass for one.
//!
//! On the device (`target_pointer_width = "32"`) a `DevAddr` is the pointer value and `ptr`, `of_ptr` and `of_fn` are plain casts: the type adds no code. On the host a pointer is 64
//! bits wide, so the conversions go through the `AddressSpace` that a test installed (`with_space`): `of_ptr` and `of_fn` give the device address of a host object, `ptr` gives the host
//! pointer of a mapped device address. An address that no mapping covers panics with the address. With no space installed the host casts like the device does (the host tests
//! that do not use the seam keep working). `ptr` gives a pointer, so the caller keeps its own access kind (plain or volatile); `mem::read32`, `write32` and `call_at` are the one
//! access path.

/// The device's 32-bit address of memory or code.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct DevAddr(u32);

impl DevAddr {
    /// The address 0: no object (a table word that nobody bound).
    pub const NULL: DevAddr = DevAddr(0);

    /// The device address with the value `a`.
    #[inline(always)]
    pub const fn new(a: u32) -> DevAddr {
        DevAddr(a)
    }

    /// The value of the address.
    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    /// The address `n` bytes on.
    #[inline(always)]
    pub const fn add(self, n: u32) -> DevAddr {
        DevAddr(self.0.wrapping_add(n))
    }
}

#[cfg(target_pointer_width = "32")]
impl DevAddr {
    /// The device address of the object `p` points to.
    #[inline(always)]
    pub fn of_ptr<T>(p: *const T) -> DevAddr {
        DevAddr(p as u32)
    }

    /// The device address of the function whose code address is `f` (`f as usize`).
    #[inline(always)]
    pub fn of_fn(f: usize) -> DevAddr {
        DevAddr(f as u32)
    }

    /// The pointer to the object at this address; the caller reads or writes it in its own way.
    #[inline(always)]
    pub fn ptr<T>(self) -> *mut T {
        self.0 as usize as *mut T
    }
}

#[cfg(not(target_pointer_width = "32"))]
mod host {
    extern crate std;
    use super::DevAddr;
    use core::cell::Cell;

    /// The host's map between device addresses and host memory and code.
    pub trait AddressSpace {
        /// The device address of the host object at `p`.
        fn of_host(&self, p: *const u8) -> Option<u32>;
        /// The device address of the host function at `f`.
        fn of_host_fn(&self, f: usize) -> u32;
        /// The host pointer of the device address `a`, or `None` when no mapping covers it.
        fn to_host(&self, a: u32) -> Option<*mut u8>;
        /// The host function at the device address `a`, or `None` when none was interned.
        fn to_host_fn(&self, a: u32) -> Option<usize>;
    }

    std::thread_local! {
        /// The space of this thread (each cargo test runs on its own thread, so tests do not see each other's space).
        static SPACE: Cell<Option<*const dyn AddressSpace>> = const { Cell::new(None) };
    }

    /// Restores the previous space when dropped, also on unwind. Private: no caller can forget it or drop it out of order.
    struct Restore(Option<*const dyn AddressSpace>);

    impl Drop for Restore {
        fn drop(&mut self) {
            SPACE.with(|c| c.set(self.0));
        }
    }

    /// Run `body` with `space` as this thread's address space, then restore the previous one (also when `body` unwinds); calls nest.
    pub fn with_space<R>(space: &dyn AddressSpace, body: impl FnOnce() -> R) -> R {
        // SAFETY: the transmute only erases the borrow's lifetime. The borrow outlives `body`, and `Restore` puts the previous pointer back before this function returns or
        // unwinds, so the stored pointer never outlives `space`. Scopes are strictly nested, so a restored pointer is the one of the enclosing (still live) scope.
        let erased: *const dyn AddressSpace =
            unsafe { core::mem::transmute::<&dyn AddressSpace, &'static dyn AddressSpace>(space) };
        let _restore = Restore(SPACE.with(|c| c.replace(Some(erased))));
        body()
    }

    /// Run `f` on the installed space, or give `None` when this thread installed none.
    fn current<R>(f: impl FnOnce(&dyn AddressSpace) -> R) -> Option<R> {
        // SAFETY: `with_space` stored the pointer and keeps its space alive until it restores the previous value.
        SPACE.with(|c| c.get()).map(|p| f(unsafe { &*p }))
    }

    impl DevAddr {
        /// The device address of the object `p` points to.
        #[inline(always)]
        pub fn of_ptr<T>(p: *const T) -> DevAddr {
            match current(|s| s.of_host(p as *const u8)) {
                Some(Some(a)) => DevAddr(a),
                Some(None) => panic!("host pointer {:#x} is in no mapped range", p as usize),
                // No space installed: cast like the device (host tests that install nothing). S2 and S3 install a map in every host test and make this arm a panic.
                None => DevAddr(p as usize as u32),
            }
        }

        /// The device address of the function whose code address is `f` (`f as usize`).
        #[inline(always)]
        pub fn of_fn(f: usize) -> DevAddr {
            match current(|s| s.of_host_fn(f)) {
                Some(a) => DevAddr(a),
                // No space installed: cast like the device (host tests that install nothing). S2 and S3 install a map in every host test and make this arm a panic.
                None => DevAddr(f as u32),
            }
        }

        /// The pointer to the object at this address. It is `*mut T` for reads too: the caller keeps its own access kind (plain or volatile). An unmapped address panics.
        #[inline(always)]
        pub fn ptr<T>(self) -> *mut T {
            match current(|s| s.to_host(self.0)) {
                Some(Some(p)) => p as *mut T,
                Some(None) => panic!("device address {:#010x} is not mapped", self.0),
                // No space installed: cast like the device (host tests that install nothing). S2 and S3 install a map in every host test and make this arm a panic.
                None => self.0 as usize as *mut T,
            }
        }

        /// The code address of the function at this address (0 for `NULL`). An address that is no interned function panics.
        #[inline(always)]
        pub(crate) fn code_addr(self) -> usize {
            if self.0 == 0 {
                return 0;
            }
            match current(|s| s.to_host_fn(self.0)) {
                Some(Some(f)) => f,
                Some(None) => panic!("device address {:#010x} is no function", self.0),
                // No space installed: cast like the device (host tests that install nothing). S2 and S3 install a map in every host test and make this arm a panic.
                None => self.0 as usize,
            }
        }
    }
}

#[cfg(not(target_pointer_width = "32"))]
pub use host::{with_space, AddressSpace};
