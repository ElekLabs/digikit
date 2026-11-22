use crate::{PackageId, Refusal, MAX_NEEDS};

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

/// What a consumer needs: the hash of a service name, the minimum packed version and whether the need is hard. A hard need refuses a bring-up when nothing
/// meets it; a soft one only orders the consumer after the provider when the graph has one that meets it. A minimum of 0 is any version. The major version of
/// the provider counts for a need that is *major checked* (the default of [`Need::new`] and [`Need::soft`]); see [`Need::any_major`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Need {
    service: u32,
    min_version: u32,
    soft: bool,
    /// Not set (the `Default`): a provider of another major version that is not older meets the need. [`dkrt_abi::version::provider_meets`].
    major_checked: bool,
}

impl Need {
    /// A hard need.
    pub const fn new(service: u32, min_version: u32) -> Need {
        Need {
            service,
            min_version,
            soft: false,
            major_checked: true,
        }
    }

    /// A soft need: the consumer loads without the service.
    pub const fn soft(service: u32, min_version: u32) -> Need {
        Need {
            service,
            min_version,
            soft: true,
            major_checked: true,
        }
    }

    /// This need is met by a provider of another major version too, when it is not older: the rule of a need whose consumer does not import the service's own
    /// functions (the minimum of the legacy needs is a runtime version, not the service's).
    pub const fn any_major(mut self) -> Need {
        self.major_checked = false;
        self
    }

    pub const fn is_hard(&self) -> bool {
        !self.soft
    }

    pub const fn service(&self) -> u32 {
        self.service
    }

    pub const fn min_version(&self) -> u32 {
        self.min_version
    }

    pub const fn major_checked(&self) -> bool {
        self.major_checked
    }
}

/// One package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    id: PackageId,
    provides: Option<Api>,
    needs: [Need; MAX_NEEDS],
    need_count: usize,
    /// The packages whose record kinds this package emits keys for, found when the graph was built from tables: it comes up after them (the owner of a kind first).
    /// Not needs: no service is attached for them, and a refused owner does not refuse this package (the rules of the core say the kind has no owner).
    owners: [PackageId; MAX_NEEDS],
    owner_count: usize,
    pinned: bool,
    /// Why this package alone cannot come up, found when the graph was built from tables (a hard need above an API of the core, a cycle, a second service of one
    /// name): [`crate::PackageGraph::bring_up_order`] refuses it with this. Such a package provides nothing and needs nothing in the graph.
    defect: Option<Refusal>,
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
                soft: false,
                major_checked: false,
            }; MAX_NEEDS],
            need_count: 0,
            owners: [PackageId(0); MAX_NEEDS],
            owner_count: 0,
            pinned: false,
            defect: None,
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

    /// This package emits keys for a record kind that `owner` declares: it comes up after `owner`. Past [`MAX_NEEDS`] owners the order is not kept (the rules of the
    /// core still refuse a kind whose owner is not up).
    pub const fn after_owner(mut self, owner: PackageId) -> Node {
        let mut i = 0;
        while i < self.owner_count {
            if self.owners[i].0 == owner.0 {
                return self;
            }
            i += 1;
        }
        if self.owner_count < MAX_NEEDS {
            self.owners[self.owner_count] = owner;
            self.owner_count += 1;
        }
        self
    }

    /// A selection entry names this package: it stays loaded without a consumer.
    pub const fn pin(mut self) -> Node {
        self.pinned = true;
        self
    }

    /// This package cannot come up for `why`: it stays a node, provides nothing and needs nothing (the first defect found stays).
    pub(crate) fn set_defect(&mut self, why: Refusal) {
        if self.defect.is_none() {
            self.defect = Some(why);
        }
    }

    /// Strips what this package provides and needs: the graph has no edge of it, so no cycle or second service through it.
    pub(crate) fn strip(&mut self) {
        self.provides = None;
        self.need_count = 0;
        self.owner_count = 0;
    }

    pub(crate) const fn defect(&self) -> Option<Refusal> {
        self.defect
    }

    /// Pin or unpin this package (the package manager changes pins as selections and requests come and go).
    pub(crate) fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
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

    /// The packages whose kinds this package emits keys for.
    pub fn kind_owners(&self) -> &[PackageId] {
        &self.owners[..self.owner_count]
    }

    pub fn needs(&self) -> &[Need] {
        &self.needs[..self.need_count.min(MAX_NEEDS)]
    }
}
