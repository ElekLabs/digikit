//! `DevAddr`: an address in the device's 32-bit space (RAM, code, MMIO, firmware tables) as its own type, so that a host pointer cannot pass for one.
//!
//! On the device (`target_pointer_width = "32"`) a `DevAddr` is the pointer value and `ptr`, `of_ptr` and `of_fn` are plain casts: the type adds no code. On the host a pointer is 64
//! bits wide, so the conversions go through the `AddressSpace` that a test installed (`with_space`): `of_ptr` and `of_fn` give the device address of a host object, `ptr` gives the host
//! pointer of a mapped device address. An address that no mapping covers panics with the address, and so does a conversion on a thread with no space installed. `ptr` gives a
//! pointer, so the caller keeps its own access kind (plain or volatile); `mem::read32`, `write32` and `call_at` are the one access path.

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

    /// The device address that module code passed as a pointer argument of the C ABI (`dk_service_declare_*`): the module runs in the device's space, so the pointer value is the
    /// address.
    #[inline(always)]
    pub fn of_abi_ptr<T>(p: *const T) -> DevAddr {
        DevAddr(p as u32)
    }
}

#[cfg(not(target_pointer_width = "32"))]
mod host {
    extern crate std;
    use super::DevAddr;
    use core::cell::Cell;
    use core::marker::PhantomData;

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

    /// The installed space of this thread until dropped; then the previous space returns, also on unwind. It borrows the space, so it cannot outlive it and, as a local, drops in
    /// the reverse order of its installs.
    pub struct SpaceGuard<'a> {
        previous: Option<*const dyn AddressSpace>,
        _space: PhantomData<&'a dyn AddressSpace>,
    }

    impl Drop for SpaceGuard<'_> {
        fn drop(&mut self) {
            SPACE.with(|c| c.set(self.previous));
        }
    }

    /// Make `space` this thread's address space until the guard drops; installs nest.
    pub fn install(space: &dyn AddressSpace) -> SpaceGuard<'_> {
        // SAFETY: the transmute only erases the borrow's lifetime. The guard carries the borrow (`'a`), so `space` outlives it, and the guard puts the previous pointer back when it
        // drops, so the stored pointer never outlives `space`. Guards drop in the reverse order of their installs, so a restored pointer is the one of a still live space.
        let erased: *const dyn AddressSpace =
            unsafe { core::mem::transmute::<&dyn AddressSpace, &'static dyn AddressSpace>(space) };
        SpaceGuard {
            previous: SPACE.with(|c| c.replace(Some(erased))),
            _space: PhantomData,
        }
    }

    /// Run `body` with `space` as this thread's address space, then restore the previous one (also when `body` unwinds); calls nest.
    pub fn with_space<R>(space: &dyn AddressSpace, body: impl FnOnce() -> R) -> R {
        let _guard = install(space);
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
            if p.is_null() {
                return DevAddr::NULL;
            }
            match current(|s| s.of_host(p as *const u8)) {
                Some(Some(a)) => DevAddr(a),
                Some(None) => panic!("host pointer {:#x} is in no mapped range", p as usize),
                None => panic!(
                    "DevAddr::of_ptr({:#x}) on a host thread with no address space installed",
                    p as usize
                ),
            }
        }

        /// The device address of the function whose code address is `f` (`f as usize`).
        #[inline(always)]
        pub fn of_fn(f: usize) -> DevAddr {
            if f == 0 {
                return DevAddr::NULL;
            }
            match current(|s| s.of_host_fn(f)) {
                Some(a) => DevAddr(a),
                None => panic!(
                    "DevAddr::of_fn({f:#x}) on a host thread with no address space installed"
                ),
            }
        }

        /// The device address that module code passed as a pointer argument of the C ABI (`dk_service_declare_*`). A value that fits 32 bits is the address itself (a host test
        /// passes an address of the image under test as the pointer value); a wider one is a host function of a service compiled for the host, and the installed space names it.
        #[inline(always)]
        pub fn of_abi_ptr<T>(p: *const T) -> DevAddr {
            if p as usize > u32::MAX as usize {
                return DevAddr::of_fn(p as usize);
            }
            DevAddr(p as usize as u32)
        }

        /// The pointer to the object at this address. It is `*mut T` for reads too: the caller keeps its own access kind (plain or volatile). The address 0 is the null pointer. An unmapped address panics.
        #[inline(always)]
        pub fn ptr<T>(self) -> *mut T {
            if self.0 == 0 {
                return core::ptr::null_mut();
            }
            match current(|s| s.to_host(self.0)) {
                Some(Some(p)) => p as *mut T,
                Some(None) => panic!("device address {:#010x} is not mapped", self.0),
                None => panic!(
                    "DevAddr::ptr({:#010x}) on a host thread with no address space installed",
                    self.0
                ),
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
                None => panic!(
                    "DevAddr::code_addr({:#010x}) on a host thread with no address space installed",
                    self.0
                ),
            }
        }
    }
}

#[cfg(not(target_pointer_width = "32"))]
pub use host::{install, with_space, AddressSpace, SpaceGuard};

/// The address of a function, kept as a runtime value (`black_box`): the compiler may not assume which function a stored pointer names.
#[inline(always)]
pub fn fn_addr(f: unsafe extern "C" fn()) -> usize {
    core::hint::black_box(f as usize)
}
