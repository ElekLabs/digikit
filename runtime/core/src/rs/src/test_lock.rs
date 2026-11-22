//! One lock for the tests that drive the runtime statics (card, arena, registry, transport buffers).
//!
//! Taking it also gives the core the package's table, as `init_once` does on the device: the host tests do not run `init_once` (it maps the arena at a device address), and a
//! test of a core with no package says so (`hooks::without_package`).
use std::sync::{LockResult, Mutex, MutexGuard};

pub(crate) struct TestLock(Mutex<()>);

impl TestLock {
    pub(crate) fn lock(&'static self) -> LockResult<MutexGuard<'static, ()>> {
        let guard = self.0.lock();
        // SAFETY: the lock is held, so no other test reads or swaps the installed table.
        unsafe { crate::hooks::install(&crate::package::hooks::HOOKS) };
        guard
    }
}

pub(crate) static LOCK: TestLock = TestLock(Mutex::new(()));
