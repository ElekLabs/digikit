//! One lock for the unit tests that touch the service's statics (`DKRT_FXS`, `DKRT_FXP`): they run one at a time.
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

/// Held for the length of a test; a test that panicked does not poison it for the next.
pub(crate) fn hold() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
