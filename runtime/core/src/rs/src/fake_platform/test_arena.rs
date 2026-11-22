//! The module arena for host tests: the arena words of the runtime state are global, the address map of `DevAddr` is per thread, so a test that touches the arena holds
//! one `TestArena` (the test lock with this thread's map of the arena), and the arena is down again when it drops.

use super::address_map::AddressMap;
use crate::test_lock::LOCK;
use core::cell::RefCell;
use dkrt_abi::dev_addr::SpaceGuard;
use std::boxed::Box;
use std::sync::MutexGuard;

/// A map of the arena installed as the address space of this thread. Dropping it uninstalls the map and clears the arena words of the state (0, as when the runtime has
/// not run `init`).
pub(crate) struct ArenaMap {
    // the guard borrows `map`; it is dropped first (`Drop`), the box never moves
    space: Option<SpaceGuard<'static>>,
    map: Box<AddressMap>,
}

impl ArenaMap {
    /// Map the arena and install the map on this thread.
    pub(crate) fn install() -> ArenaMap {
        let map = Box::new(AddressMap::new());
        map.map_arena();
        // SAFETY: the box is owned by the returned value and outlives the guard, which `drop` releases first.
        let space = unsafe { core::mem::transmute::<SpaceGuard<'_>, SpaceGuard<'static>>(map.install()) };
        ArenaMap { space: Some(space), map }
    }
}

impl Drop for ArenaMap {
    fn drop(&mut self) {
        unsafe {
            let s = crate::st();
            s.arena_lo = 0;
            s.arena_hi = 0;
            s.free_head = 0;
        }
        self.space.take();
        let _ = &self.map;
    }
}

/// The test lock and the arena map of this thread, for the scope of a test. The arena is empty and uninitialised until `init`.
pub(crate) struct TestArena {
    // the map goes first, then the lock is released
    _map: ArenaMap,
    _lock: MutexGuard<'static, ()>,
}

impl TestArena {
    pub(crate) fn lock() -> TestArena {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TestArena { _map: ArenaMap::install(), _lock: lock }
    }

    /// Initialise the arena (empty, all free); again after a test zeroed the state.
    pub(crate) fn init(&self) {
        unsafe { crate::arena::init() }
    }
}

std::thread_local! {
    /// The map of a scripted DSP link, for the tests that already hold the test lock (`script_link`); `script_end` removes it.
    static SCRIPTED: RefCell<Option<ArenaMap>> = const { RefCell::new(None) };
}

/// For a test that holds the test lock itself: bring the arena up (map and `init`) unless this thread's scripted arena is up already.
pub(crate) fn script_up() {
    SCRIPTED.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            *s = Some(ArenaMap::install());
            unsafe { crate::arena::init() }
        }
    });
}

pub(crate) fn script_down() {
    SCRIPTED.with(|s| s.borrow_mut().take());
}
