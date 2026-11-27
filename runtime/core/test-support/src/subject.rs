//! The owner object under test, for the fixture functions of the contract suite. A fixture's `init` (a module's builder calls) and `state` (a view of the service's tables) are
//! plain functions: they get no handle to the instance that the driver started, so the driver publishes it here for its lifetime. `subject::<T>(|s| ...)` runs a closure on
//! it. The registry is per thread (each test runs on its own) and a guard puts the previous entry back when the driver goes, also when a test panics.
use core::any::TypeId;
use core::cell::Cell;
use core::ffi::c_void;

#[derive(Clone, Copy)]
struct Entry {
    id: TypeId,
    instance: *mut c_void,
}

std::thread_local! {
    static CURRENT: Cell<Option<Entry>> = const { Cell::new(None) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// The published instance of a started driver; the previous one is current again when the guard drops.
pub(crate) struct SubjectGuard {
    previous: Option<Entry>,
}

impl SubjectGuard {
    /// Publish `instance` as the subject of type `T`.
    pub(crate) fn publish<T: 'static>(instance: *mut T) -> SubjectGuard {
        let entry = Entry {
            id: TypeId::of::<T>(),
            instance: instance as *mut c_void,
        };
        SubjectGuard {
            previous: CURRENT.with(|c| c.replace(Some(entry))),
        }
    }
}

impl Drop for SubjectGuard {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.previous));
    }
}

/// Run `f` on the owner object of type `T` that the suite's driver started on this thread. Panics when there is none or it is another type, or when `f` calls `subject` again
/// (a second `&mut` to the same instance).
pub fn subject<T: 'static, R>(f: impl FnOnce(&mut T) -> R) -> R {
    let entry = CURRENT
        .with(|c| c.get())
        .filter(|e| e.id == TypeId::of::<T>())
        .unwrap_or_else(|| panic!("no started {} on this thread", core::any::type_name::<T>()));
    assert!(!BUSY.with(|b| b.replace(true)), "subject() nested");
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            BUSY.with(|b| b.set(false));
        }
    }
    let _release = Release;
    // SAFETY: the instance is the `T` a started driver published; the guard keeps the entry only while the driver lives, and `BUSY` keeps this the only reference.
    f(unsafe { &mut *(entry.instance as *mut T) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subject_is_the_published_instance_until_its_guard_drops_and_the_previous_one_comes_back()
    {
        let (mut a, mut b) = (1u32, 2u32);
        let outer = SubjectGuard::publish(&mut a);
        assert_eq!(subject::<u32, _>(|v| *v), 1);
        {
            let _inner = SubjectGuard::publish(&mut b);
            subject::<u32, _>(|v| *v += 10);
            assert_eq!(subject::<u32, _>(|v| *v), 12);
        }
        assert_eq!(subject::<u32, _>(|v| *v), 1);
        drop(outer);
        assert!(std::panic::catch_unwind(|| subject::<u32, _>(|v| *v)).is_err());
    }

    #[test]
    fn a_subject_of_another_type_or_a_nested_call_is_refused_and_the_registry_recovers() {
        let mut a = 5u32;
        let _g = SubjectGuard::publish(&mut a);
        assert!(std::panic::catch_unwind(|| subject::<u64, _>(|v| *v)).is_err());
        assert!(
            std::panic::catch_unwind(|| subject::<u32, _>(|_| subject::<u32, _>(|v| *v))).is_err()
        );
        assert_eq!(
            subject::<u32, _>(|v| *v),
            5,
            "a refused nesting leaves the cell usable"
        );
    }
}
