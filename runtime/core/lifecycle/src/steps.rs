use crate::{PackageId, MAX_PACKAGES};

/// An ordered list of packages: the steps of a bring-up or a take-down.
#[derive(Clone, Copy, Debug)]
pub struct Steps {
    ids: [PackageId; MAX_PACKAGES],
    len: usize,
}

impl Steps {
    pub const fn new() -> Steps {
        Steps {
            ids: [PackageId(0); MAX_PACKAGES],
            len: 0,
        }
    }

    pub(crate) fn push(&mut self, id: PackageId) {
        // callers push each package of a graph at most once
        if self.len < MAX_PACKAGES {
            self.ids[self.len] = id;
            self.len += 1;
        }
    }

    /// Drops the last step.
    pub(crate) fn pop(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    /// Drops `id`, keeping the order of the others.
    pub(crate) fn remove(&mut self, id: PackageId) {
        if let Some(i) = self.as_slice().iter().position(|&x| x == id) {
            self.ids.copy_within(i + 1..self.len, i);
            self.len -= 1;
        }
    }

    pub(crate) fn contains(&self, id: PackageId) -> bool {
        self.as_slice().contains(&id)
    }

    pub fn as_slice(&self) -> &[PackageId] {
        &self.ids[..self.len]
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [PackageId] {
        &mut self.ids[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for Steps {
    fn default() -> Self {
        Steps::new()
    }
}

impl PartialEq for Steps {
    fn eq(&self, other: &Steps) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for Steps {}

impl PartialEq<[PackageId]> for Steps {
    fn eq(&self, other: &[PackageId]) -> bool {
        self.as_slice() == other
    }
}

impl<const N: usize> PartialEq<[PackageId; N]> for Steps {
    fn eq(&self, other: &[PackageId; N]) -> bool {
        self.as_slice() == other.as_slice()
    }
}

/// Iterates the packages of a [`Steps`] by value.
pub struct StepsIter {
    steps: Steps,
    at: usize,
}

impl Iterator for StepsIter {
    type Item = PackageId;
    fn next(&mut self) -> Option<PackageId> {
        let id = *self.steps.as_slice().get(self.at)?;
        self.at += 1;
        Some(id)
    }
}

impl IntoIterator for Steps {
    type Item = PackageId;
    type IntoIter = StepsIter;
    fn into_iter(self) -> StepsIter {
        StepsIter { steps: self, at: 0 }
    }
}

/// Ascending by id (insertion sort: at most [`MAX_PACKAGES`] entries).
pub(crate) fn sort(s: &mut Steps) {
    for i in 1..s.len {
        let mut j = i;
        while j > 0 && s.ids[j - 1] > s.ids[j] {
            s.ids.swap(j - 1, j);
            j -= 1;
        }
    }
}
