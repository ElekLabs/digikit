use super::Probe;
use crate::install::Entry;
use crate::{Api, Manifest, PackageId};
use dkneeds::keys::{name_hash, write_table, KeyGroup, KindDecl, Need as KNeed, Pair, Table};
use dkrt_abi::provider::Status;
use dkrt_fit::ledger::{Aggregate, Owner, OwnerId, PoolKey, ResourceDecl, Want};
use std::vec;
use std::vec::Vec;

/// A package in a line: `Pkg::service(1, "ref", V1).resource("slots", 4)`, `Pkg::module(2).needs("ref", V1).wants(1, "slots", 2)`.
#[derive(Clone)]
pub struct Pkg {
    pub(super) id: PackageId,
    provides: Option<(&'static str, u32)>,
    needs: Vec<(&'static str, u32, bool)>,
    /// The record kinds this package declares (`KINDS`) and the kinds it has keys for (`KEYS`).
    owns: Vec<u32>,
    emits: Vec<u32>,
    pub(super) resources: Vec<ResourceDecl>,
    wants: Vec<Want>,
    pub(super) dsp: bool,
    pub(super) rank: Option<u32>,
}

impl Pkg {
    pub fn module(id: u8) -> Pkg {
        Pkg {
            id: PackageId(id),
            provides: None,
            needs: Vec::new(),
            owns: Vec::new(),
            emits: Vec::new(),
            resources: Vec::new(),
            wants: Vec::new(),
            dsp: false,
            rank: None,
        }
    }

    /// A service of this name and packed version.
    pub fn service(id: u8, name: &'static str, version: u32) -> Pkg {
        Pkg {
            provides: Some((name, version)),
            ..Pkg::module(id)
        }
    }

    /// A hard need.
    pub fn needs(mut self, service: &'static str, min_version: u32) -> Pkg {
        self.needs.push((service, min_version, true));
        self
    }

    /// A soft need: the package loads without the service.
    pub fn soft_needs(mut self, service: &'static str, min_version: u32) -> Pkg {
        self.needs.push((service, min_version, false));
        self
    }

    /// This package declares record kind `kind`.
    pub fn owns_kind(mut self, kind: u32) -> Pkg {
        self.owns.push(kind);
        self
    }

    /// This package has keys for record kind `kind`.
    pub fn emits_kind(mut self, kind: u32) -> Pkg {
        self.emits.push(kind);
        self
    }

    /// This service's callbacks run at `rank` (the host's `attach_rank`; the package id when unset).
    pub fn with_rank(mut self, rank: u32) -> Pkg {
        self.rank = Some(rank);
        self
    }

    /// This package has a DSP part: its host's `start_dsp` answers `true`.
    pub fn with_dsp(mut self) -> Pkg {
        self.dsp = true;
        self
    }

    /// A pool this package declares, summed, with this limit.
    pub fn resource(mut self, name: &str, limit: u32) -> Pkg {
        self.resources
            .push(ResourceDecl::new(name_hash(name), limit, Aggregate::Sum));
        self
    }

    /// This package claims `units` of the pool `name` that package `owner` declares.
    pub fn wants(mut self, owner: u8, name: &str, units: u32) -> Pkg {
        let key = PoolKey::new(Owner::Package(OwnerId(owner)), name_hash(name));
        self.wants.push(Want::new(key, units));
        self
    }
}

/// The header tables, pools, wants and entries of a list of [`Pkg`].
pub struct FakeManifest {
    tables: Vec<(PackageId, &'static [u8])>,
    resources: Vec<(PackageId, Vec<ResourceDecl>)>,
    wants: Vec<(PackageId, Vec<Want>)>,
    probe: Option<Probe>,
}

impl FakeManifest {
    pub fn new(pkgs: &[Pkg]) -> FakeManifest {
        let mut m = FakeManifest {
            tables: Vec::new(),
            resources: Vec::new(),
            wants: Vec::new(),
            probe: None,
        };
        for p in pkgs {
            // a test fake: the bytes live as long as the test process
            let bytes: &'static [u8] = vec::Vec::leak(table_of(p));
            m.tables.push((p.id, bytes));
            m.resources.push((p.id, p.resources.clone()));
            m.wants.push((p.id, p.wants.clone()));
        }
        m
    }
}

impl FakeManifest {
    /// `admit` records into `probe` (`Call::Admit`) and fails as the probe's faults say.
    pub fn with_probe(mut self, probe: Probe) -> FakeManifest {
        self.probe = Some(probe);
        self
    }

    /// `pkg` joins the manifest, or replaces the package of its id (a file that was added or replaced since the manager read the tables).
    pub fn put(&mut self, pkg: &Pkg) {
        let bytes: &'static [u8] = vec::Vec::leak(table_of(pkg));
        self.tables.retain(|(id, _)| *id != pkg.id);
        self.tables.push((pkg.id, bytes));
        self.resources.retain(|(id, _)| *id != pkg.id);
        self.resources.push((pkg.id, pkg.resources.clone()));
        self.wants.retain(|(id, _)| *id != pkg.id);
        self.wants.push((pkg.id, pkg.wants.clone()));
    }

    /// The pools and wants of `pkg` become those of the package of the same id (its header tables stay): a package whose next build claims differently.
    pub fn replace_claims(&mut self, pkg: &Pkg) {
        for (_, r) in self.resources.iter_mut().filter(|(id, _)| *id == pkg.id) {
            *r = pkg.resources.clone();
        }
        for (_, w) in self.wants.iter_mut().filter(|(id, _)| *id == pkg.id) {
            *w = pkg.wants.clone();
        }
    }
}

fn table_of(p: &Pkg) -> Vec<u8> {
    let provides: Vec<Pair> = p
        .provides
        .iter()
        .map(|(n, v)| Pair {
            a: name_hash(n),
            b: *v,
        })
        .collect();
    let needs: Vec<KNeed> = p
        .needs
        .iter()
        .map(|&(n, min, hard)| KNeed {
            service: name_hash(n),
            min_version: min,
            hard,
        })
        .collect();
    let kinds: Vec<KindDecl> = p
        .owns
        .iter()
        .map(|&kind| KindDecl {
            kind,
            len: 16,
            entry: 8,
        })
        .collect();
    let keys: Vec<KeyGroup> = p
        .emits
        .iter()
        .map(|&kind| KeyGroup { kind, keys: &[5] })
        .collect();
    let t = Table {
        needs: &needs,
        provides: &provides,
        kinds: &kinds,
        keys: &keys,
        ..Table::default()
    };
    let mut out = vec![0u8; 512];
    let n = write_table(&t, &mut out).expect("a fake header table fits 512 bytes");
    out.truncate(n);
    out
}

impl Manifest for FakeManifest {
    fn tables(&self) -> &[(PackageId, &[u8])] {
        &self.tables
    }

    fn core_apis(&self) -> &[Api] {
        &[]
    }

    fn core_resources(&self) -> &[ResourceDecl] {
        &[]
    }

    fn resources(&self, package: PackageId) -> &[ResourceDecl] {
        self.resources
            .iter()
            .find(|(p, _)| *p == package)
            .map_or(&[], |(_, r)| r)
    }

    fn wants(&self, package: PackageId) -> &[Want] {
        self.wants
            .iter()
            .find(|(p, _)| *p == package)
            .map_or(&[], |(_, w)| w)
    }

    fn entries(&self, _package: PackageId) -> &[Entry] {
        &[]
    }

    fn admit(&mut self, package: PackageId) -> Result<(), Status> {
        match &self.probe {
            Some(probe) => probe.record_admit(package),
            None => Ok(()),
        }
    }
}
