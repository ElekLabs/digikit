use super::LifecycleError;
use crate::{PackageId, MAX_PACKAGES};
use core::fmt;

/// A package that an operation did not bring up, and why: its own failure, or the refusal of a service it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub package: PackageId,
    pub reason: LifecycleError,
}

const FILLER: Refused = Refused {
    package: PackageId(0),
    reason: LifecycleError::NotLive {
        package: PackageId(0),
    },
};

/// The packages an operation refused, in the order it met them: a cause comes before its consumers.
#[derive(Clone, Copy)]
pub(super) struct RefusedList {
    items: [Refused; MAX_PACKAGES],
    len: usize,
}

impl RefusedList {
    pub(super) const fn new() -> RefusedList {
        RefusedList {
            items: [FILLER; MAX_PACKAGES],
            len: 0,
        }
    }

    /// A package is refused at most once; a graph never holds more packages than slots.
    pub(super) fn push(&mut self, package: PackageId, reason: LifecycleError) {
        if self.len < MAX_PACKAGES && !self.contains(package) {
            self.items[self.len] = Refused { package, reason };
            self.len += 1;
        }
    }

    pub(super) fn contains(&self, package: PackageId) -> bool {
        self.as_slice().iter().any(|r| r.package == package)
    }

    pub(super) fn as_slice(&self) -> &[Refused] {
        &self.items[..self.len]
    }
}

impl Default for RefusedList {
    fn default() -> Self {
        RefusedList::new()
    }
}

impl PartialEq for RefusedList {
    fn eq(&self, other: &RefusedList) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for RefusedList {}

impl fmt::Debug for RefusedList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}
