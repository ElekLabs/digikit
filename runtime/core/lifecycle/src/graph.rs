use crate::node::{Need, Node};
use crate::refusal::Refusal;
use crate::steps::{sort, Steps};
use crate::{PackageId, MAX_NEEDS, MAX_PACKAGES};
use dkrt_abi::version::version_satisfies;

mod bring_up;
mod from_tables;
mod pin;
mod take_down;

/// How a need resolves against the services of the graph.
enum Resolved {
    Met(PackageId),
    Incompatible,
    None,
}

/// The packages and their needs.
#[derive(Clone, Debug)]
pub struct PackageGraph {
    nodes: [Node; MAX_PACKAGES],
    len: usize,
}

impl PackageGraph {
    /// A graph of `nodes`. Refuses more than [`MAX_PACKAGES`], a package with more than [`MAX_NEEDS`] needs, a repeated id, two services of
    /// one name and major version and a cycle of needs. A graph is acyclic by construction, so every walk over it ends. A need that nothing meets is no
    /// edge here: [`PackageGraph::bring_up_order`] refuses it.
    pub fn from_nodes(nodes: &[Node]) -> Result<PackageGraph, Refusal> {
        if nodes.len() > MAX_PACKAGES {
            return Err(Refusal::TooManyPackages);
        }
        let mut g = PackageGraph {
            nodes: [Node::new(PackageId(0)); MAX_PACKAGES],
            len: 0,
        };
        for (i, n) in nodes.iter().enumerate() {
            if n.need_count() > MAX_NEEDS {
                return Err(Refusal::TooManyNeeds { package: n.id() });
            }
            for m in &nodes[..i] {
                if m.id() == n.id() {
                    return Err(Refusal::DuplicatePackage { package: n.id() });
                }
                if let (Some(a), Some(b)) = (m.provides(), n.provides()) {
                    if a.name() == b.name() && a.version() >> 16 == b.version() >> 16 {
                        return Err(Refusal::DuplicateService {
                            package: n.id(),
                            service: a.name(),
                        });
                    }
                }
            }
            g.nodes[i] = *n;
        }
        g.len = nodes.len();
        g.refuse_cycle()?;
        Ok(g)
    }

    /// The one cycle check: depth first from each package over the needs that a service meets. `path` holds the packages being visited.
    fn refuse_cycle(&self) -> Result<(), Refusal> {
        let mut done = Steps::new();
        for n in self.nodes() {
            let mut path = Steps::new();
            self.walk(n.id(), &mut done, &mut path)?;
        }
        Ok(())
    }

    fn walk(&self, p: PackageId, done: &mut Steps, path: &mut Steps) -> Result<(), Refusal> {
        if done.contains(p) {
            return Ok(());
        }
        if path.contains(p) {
            return Err(Refusal::Cycle { package: p });
        }
        path.push(p);
        for s in self.services_of(p) {
            self.walk(s, done, path)?;
        }
        path.pop();
        done.push(p);
        Ok(())
    }

    fn nodes(&self) -> &[Node] {
        &self.nodes[..self.len]
    }

    pub fn node(&self, id: PackageId) -> Option<&Node> {
        self.nodes().iter().find(|n| n.id() == id)
    }

    /// The package that meets `need`. Of several compatible services (two majors never both meet one need) the lowest id.
    fn resolve(&self, need: &Need) -> Resolved {
        let mut found = Resolved::None;
        for n in self.nodes() {
            let Some(api) = n.provides() else { continue };
            if api.name() != need.service() {
                continue;
            }
            if version_satisfies(api.version(), need.min_version()) {
                match found {
                    Resolved::Met(id) if id <= n.id() => {}
                    _ => found = Resolved::Met(n.id()),
                }
            } else if matches!(found, Resolved::None) {
                found = Resolved::Incompatible;
            }
        }
        found
    }

    /// The services package `p` needs and that are in the graph, by ascending id. A need that nothing meets is left out
    /// ([`PackageGraph::bring_up_order`] refuses it).
    pub fn services_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        if let Some(node) = self.node(p) {
            for need in node.needs() {
                if let Resolved::Met(id) = self.resolve(need) {
                    if !out.contains(id) {
                        out.push(id);
                    }
                }
            }
        }
        sort(&mut out);
        out.into_iter()
    }

    /// The packages that need service `p`, by ascending id.
    pub fn consumers_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        for n in self.nodes() {
            for need in n.needs() {
                if matches!(self.resolve(need), Resolved::Met(id) if id == p) {
                    out.push(n.id());
                    break;
                }
            }
        }
        sort(&mut out);
        out.into_iter()
    }
}

#[cfg(test)]
mod tests;
