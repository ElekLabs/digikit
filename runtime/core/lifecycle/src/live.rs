use crate::{PackageId, MAX_PACKAGES};

/// The packages that are up. Insertion order does not matter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveSet {
    ids: [PackageId; MAX_PACKAGES],
    len: usize,
}

impl LiveSet {
    pub const fn new() -> LiveSet {
        LiveSet {
            ids: [PackageId(0); MAX_PACKAGES],
            len: 0,
        }
    }

    pub fn of(ids: &[PackageId]) -> LiveSet {
        let mut s = LiveSet::new();
        for &id in ids {
            s.insert(id);
        }
        s
    }

    pub fn contains(&self, id: PackageId) -> bool {
        self.ids[..self.len].contains(&id)
    }

    /// False when the set is full (a graph never holds more packages than [`MAX_PACKAGES`], so a live set of its packages is never full).
    pub fn insert(&mut self, id: PackageId) -> bool {
        if self.contains(id) {
            return true;
        }
        if self.len == MAX_PACKAGES {
            return false;
        }
        self.ids[self.len] = id;
        self.len += 1;
        true
    }

    pub fn remove(&mut self, id: PackageId) {
        if let Some(i) = self.ids[..self.len].iter().position(|&x| x == id) {
            self.ids[i] = self.ids[self.len - 1];
            self.len -= 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for LiveSet {
    fn default() -> Self {
        LiveSet::new()
    }
}
