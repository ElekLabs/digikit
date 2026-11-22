use super::PackageGraph;
use crate::node::{Api, Need, Node};
use crate::refusal::Refusal;
use crate::{PackageId, MAX_PACKAGES};
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use dkneeds::keys::{tag, Keys};
use dkrt_abi::version::core_meets;

impl PackageGraph {
    /// The graph of the packages whose header tables are given (`dkneeds` reads them: a service's `PROVIDES` is its API, the `NEEDS` are the needs).
    ///
    /// `core_apis` are the APIs the core and the image provide without a package (the `runtime` floor and the services built in). A need on one of them
    /// leaves no edge: a hard need of a version newer than the API's is [`Refusal::Incompatible`] for its package, a soft one is dropped. Every other need is
    /// resolved against the services of the graph (a need of 0 is any version; a need whose consumer imports the service's functions, a `SERVICE_IMPORTS` item, also
    /// keeps the major version), so a hard need that nothing meets is refused later by [`PackageGraph::bring_up_order`]. A package that emits keys for a record
    /// kind (`KEYS`) comes up after the package that declares it (`KINDS`); a service only after services.
    ///
    /// A table that does not parse, or a package with more than one `PROVIDES` item, is [`Refusal::BadTable`]; the other refusals are those of
    /// [`PackageGraph::from_nodes`], except that a hard need above an API of the core, a cycle and a second service of one name refuse the package alone: it is a
    /// node with a defect that [`PackageGraph::bring_up_order`] reports for it, and a cycle or a duplicate leaves the package without edges.
    pub fn from_tables(
        packages: &[(PackageId, &[u8])],
        core_apis: &[Api],
    ) -> Result<PackageGraph, Refusal> {
        let mut g = MaybeUninit::<PackageGraph>::uninit();
        // SAFETY: `init_from_tables` writes the whole graph before it answers, also when it refuses
        unsafe {
            PackageGraph::init_from_tables(g.as_mut_ptr(), packages, core_apis)?;
            Ok(g.assume_init())
        }
    }

    /// [`PackageGraph::from_tables`] into the graph at `out`, with no copy of it on the stack (a graph is 1.6 KB; the boot's stack is shared with the firmware). A refusal leaves
    /// the graph at `out` empty or unusable: callers that keep the old graph build into a place of their own first.
    ///
    /// # Safety
    /// `out` is valid for writes of a graph and not read while this runs.
    pub unsafe fn init_from_tables(
        out: *mut PackageGraph,
        packages: &[(PackageId, &[u8])],
        core_apis: &[Api],
    ) -> Result<(), Refusal> {
        // slot by slot: a copy of a constant would be built on the stack first
        let nodes = addr_of_mut!((*out).nodes).cast::<Node>();
        for i in 0..MAX_PACKAGES {
            nodes.add(i).write(Node::new(PackageId(0)));
        }
        addr_of_mut!((*out).len).write(0);
        let g = &mut *out;
        if packages.len() > MAX_PACKAGES {
            return Err(Refusal::TooManyPackages);
        }
        for (i, (id, bytes)) in packages.iter().enumerate() {
            g.nodes[i] = node_of(*id, bytes, core_apis)?;
        }
        g.len = packages.len();
        g.link_kind_owners(packages);
        // the packages of a cycle, or a second service of one name, are refused alone: they are marked and stripped of their edges, so that the others still load (a
        // consumer of one then finds its service unmet). The other refusals are the graph's.
        loop {
            match g.validate() {
                Err(Refusal::Cycle { package }) => g.mark_cycle(package),
                Err(why @ Refusal::DuplicateService { package, .. }) => g.mark_defect(package, why),
                other => return other,
            }
        }
    }

    /// Gives each package that emits keys for a record kind an edge to the packages that declare the kind. The packages are the same as `nodes`, in the same order.
    fn link_kind_owners(&mut self, packages: &[(PackageId, &[u8])]) {
        for (i, (_, bytes)) in packages.iter().enumerate() {
            let Ok(emitter) = Keys::parse(bytes) else {
                continue;
            };
            let mut gi = 0;
            while let Some((kind, _)) = emitter.group(gi) {
                gi += 1;
                for (j, (owner, theirs)) in packages.iter().enumerate() {
                    // a service waits for services only: all of them come up before the modules
                    let allowed = j != i
                        && (self.nodes[i].provides().is_none()
                            || self.nodes[j].provides().is_some());
                    let declares = Keys::parse(theirs).is_ok_and(|k| {
                        (0..k.count(tag::KINDS))
                            .any(|n| k.words(tag::KINDS, n).is_some_and(|w| w.0 == kind))
                    });
                    if allowed && declares {
                        self.nodes[i] = self.nodes[i].after_owner(*owner);
                    }
                }
            }
        }
    }
}

fn node_of(id: PackageId, bytes: &[u8], core_apis: &[Api]) -> Result<Node, Refusal> {
    let bad = Refusal::BadTable { package: id };
    let keys = Keys::parse(bytes).map_err(|_| bad)?;
    let mut node = Node::new(id);
    match keys.count(tag::PROVIDES) {
        0 => {}
        1 => {
            let (service, version, _) = keys.words(tag::PROVIDES, 0).ok_or(bad)?;
            node = node.providing(Api::new(service, version));
        }
        _ => return Err(bad),
    }
    for need in keys.needs() {
        // an import of the service's own functions keeps the major version of its provider; the other needs carry a runtime version
        let checked = (0..keys.count(tag::SERVICE_IMPORTS)).any(|i| {
            keys.words(tag::SERVICE_IMPORTS, i)
                .is_some_and(|w| w.1 == need.service)
        });
        let core = core_apis.iter().find(|a| a.name() == need.service);
        match core {
            Some(a) if core_meets(a.version(), need.min_version) => continue,
            Some(_) if need.hard => {
                node.set_defect(Refusal::Incompatible {
                    package: id,
                    service: need.service,
                });
                continue;
            }
            Some(_) => continue,
            None => {}
        }
        let n = if need.hard {
            Need::new(need.service, need.min_version)
        } else {
            Need::soft(need.service, need.min_version)
        };
        node = node.needing(if checked { n } else { n.any_major() });
    }
    Ok(node)
}

#[cfg(test)]
mod tests;
