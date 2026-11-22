//! One lock for the tests that drive the runtime statics (card, arena, registry, transport buffers).
//!
//! `lock` only takes the lock. `lock_with_package` also gives the core the package's table, as `init_once` does on the device (the host tests do not run
//! `init_once`: it maps the arena at a device address); a test of a core with no package takes the plain lock, or runs inside `hooks::without_package`.
use std::sync::{LockResult, Mutex, MutexGuard};

pub(crate) struct TestLock(Mutex<()>);

impl TestLock {
    pub(crate) fn lock(&'static self) -> LockResult<MutexGuard<'static, ()>> {
        self.0.lock()
    }

    /// The lock, with the package installed for the scope of the guard's test.
    pub(crate) fn lock_with_package(&'static self) -> LockResult<MutexGuard<'static, ()>> {
        let guard = self.0.lock();
        // SAFETY: the lock is held, so no other test reads or swaps the installed table.
        let _ = crate::platform::TEST_INFO.set(|h| unsafe { crate::package::cleanup_tests::test_info(h) });
        unsafe {
            crate::package::resident::install(&crate::resident::TABLE).expect("the resident table fits the package");
            crate::hooks::install(&crate::package::hooks::HOOKS);
        }
        guard
    }
}

pub(crate) static LOCK: TestLock = TestLock(Mutex::new(()));
