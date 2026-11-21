use crate::{PackageId, MAX_NEEDS};

/// What a service provides: the hash of its name and its packed version (`major << 16 | minor << 8 | patch`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Api {
    name: u32,
    version: u32,
}

impl Api {
    pub const fn new(name: u32, version: u32) -> Api {
        Api { name, version }
    }

    pub const fn name(&self) -> u32 {
        self.name
    }

    pub const fn version(&self) -> u32 {
        self.version
    }
}

/// What a consumer needs: the hash of a service name and the minimum packed version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Need {
    service: u32,
    min_version: u32,
}

impl Need {
    pub const fn new(service: u32, min_version: u32) -> Need {
        Need {
            service,
            min_version,
        }
    }

    pub const fn service(&self) -> u32 {
        self.service
    }

    pub const fn min_version(&self) -> u32 {
        self.min_version
    }
}

/// One package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    id: PackageId,
    provides: Option<Api>,
    needs: [Need; MAX_NEEDS],
    need_count: usize,
    pinned: bool,
}

impl Node {
    /// A module with no needs.
    pub const fn new(id: PackageId) -> Node {
        Node {
            id,
            provides: None,
            needs: [Need {
                service: 0,
                min_version: 0,
            }; MAX_NEEDS],
            need_count: 0,
            pinned: false,
        }
    }

    /// This package provides `api`: it is a service.
    pub const fn providing(mut self, api: Api) -> Node {
        self.provides = Some(api);
        self
    }

    /// This package needs `need`. Needs past [`MAX_NEEDS`] are counted but refused by [`crate::PackageGraph::from_nodes`].
    pub const fn needing(mut self, need: Need) -> Node {
        if self.need_count < MAX_NEEDS {
            self.needs[self.need_count] = need;
        }
        self.need_count += 1;
        self
    }

    /// A selection entry names this package: it stays loaded without a consumer.
    pub const fn pin(mut self) -> Node {
        self.pinned = true;
        self
    }

    pub const fn id(&self) -> PackageId {
        self.id
    }

    pub const fn provides(&self) -> Option<Api> {
        self.provides
    }

    pub const fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// How many needs were added, including those past [`MAX_NEEDS`].
    pub(crate) const fn need_count(&self) -> usize {
        self.need_count
    }

    pub fn needs(&self) -> &[Need] {
        &self.needs[..self.need_count.min(MAX_NEEDS)]
    }
}
