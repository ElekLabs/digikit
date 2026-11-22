use crate::node::{Need, Node};
use crate::refusal::Refusal;
use crate::steps::{sort, Steps};
use crate::{PackageId, MAX_NEEDS, MAX_PACKAGES};
use dkrt_abi::version::provider_meets;

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

/// No packages: what a graph is built from, in place (`PackageGraph::init_from_tables`).
const EMPTY: PackageGraph = PackageGraph {
    nodes: [Node::new(PackageId(0)); MAX_PACKAGES],
    len: 0,
};

/// The packages and their needs.
#[derive(Clone, Debug)]
pub struct PackageGraph {
    nodes: [Node; MAX_PACKAGES],
    len: usize,
}

impl PackageGraph {
    /// Marks package `id` as one that cannot come up for `why` and strips its edges (a refusal that is the package's alone, not the graph's).
    pub(crate) fn mark_defect(&mut self, id: PackageId, why: Refusal) {
        if let Some(n) = self.nodes[..self.len].iter_mut().find(|n| n.id() == id) {
            n.set_defect(why);
            n.strip();
        }
    }

    /// A graph of `nodes`. Refuses more than [`MAX_PACKAGES`], a package with more than [`MAX_NEEDS`] needs, a repeated id, two services of
    /// one name and major version and a cycle of needs. A graph is acyclic by construction, so every walk over it ends. A need that nothing meets is no
    /// edge here: [`PackageGraph::bring_up_order`] refuses it.
    pub fn from_nodes(nodes: &[Node]) -> Result<PackageGraph, Refusal> {
        let mut g = EMPTY;
        g.fill(nodes)?;
        Ok(g)
    }

    /// Takes `nodes` and checks them (the refusals of [`PackageGraph::from_nodes`]); a refusal leaves the graph unusable.
    fn fill(&mut self, nodes: &[Node]) -> Result<(), Refusal> {
        if nodes.len() > MAX_PACKAGES {
            return Err(Refusal::TooManyPackages);
        }
        self.nodes[..nodes.len()].copy_from_slice(nodes);
        self.len = nodes.len();
        self.validate()
    }

    /// The refusals of [`PackageGraph::from_nodes`] over the nodes held.
    fn validate(&self) -> Result<(), Refusal> {
        let nodes = self.nodes();
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
        }
        self.refuse_cycle()
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
        for s in self.ordering_of(p) {
            self.walk(s, done, path)?;
        }
        path.pop();
        done.push(p);
        Ok(())
    }

    pub(crate) fn nodes(&self) -> &[Node] {
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
            if provider_meets(api.version(), need.min_version(), need.major_checked()) {
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

    /// The services package `p` needs and that are in the graph, by ascending id: what it is attached to. A need that nothing meets is left out
    /// ([`PackageGraph::bring_up_order`] refuses it).
    pub fn services_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        self.met_needs_of(p, false)
    }

    /// The services `p` needs with a hard need and that are in the graph, by ascending id: the ones whose refusal refuses `p`. A soft need never does.
    pub fn hard_services_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        self.met_needs_of(p, true)
    }

    fn met_needs_of(&self, p: PackageId, hard_only: bool) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        if let Some(node) = self.node(p) {
            for need in node.needs() {
                if hard_only && !need.is_hard() {
                    continue;
                }
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

    /// What `p` comes up after, by ascending id: the services it needs and the packages that own the record kinds it emits keys for. The order of every walk.
    pub fn dependencies_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        for s in self.services_of(p) {
            out.push(s);
        }
        if let Some(node) = self.node(p) {
            for &o in node.kind_owners() {
                if !out.contains(o) {
                    out.push(o);
                }
            }
        }
        sort(&mut out);
        out.into_iter()
    }

    /// What `p` must come up after: its hard needs and its kind owners. Only these edges can make a cycle; a soft edge orders a bring-up when it can and
    /// is skipped when it would close a loop.
    fn ordering_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        for s in self.hard_services_of(p) {
            out.push(s);
        }
        if let Some(node) = self.node(p) {
            for &o in node.kind_owners() {
                if !out.contains(o) {
                    out.push(o);
                }
            }
        }
        sort(&mut out);
        out.into_iter()
    }

    /// The packages that come up after `p` (they need service `p` or emit keys for a kind it owns), by ascending id.
    pub fn consumers_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> {
        let mut out = Steps::new();
        for n in self.nodes() {
            let needs = n
                .needs()
                .iter()
                .any(|need| matches!(self.resolve(need), Resolved::Met(id) if id == p));
            if needs || n.kind_owners().contains(&p) {
                out.push(n.id());
            }
        }
        sort(&mut out);
        out.into_iter()
    }

    /// Whether `from` comes up after `to` through hard needs and kind owners, directly or through other packages.
    pub(crate) fn depends_on(&self, from: PackageId, to: PackageId) -> bool {
        let mut seen = Steps::new();
        let mut todo = Steps::new();
        todo.push(from);
        while let Some(p) = todo.as_slice().last().copied() {
            todo.pop();
            for d in self.ordering_of(p) {
                if d == to {
                    return true;
                }
                if !seen.contains(d) {
                    seen.push(d);
                    todo.push(d);
                }
            }
        }
        false
    }

    /// Marks every package on a cycle through `at` as one that cannot come up (the packages that need each other are all refused, none is left to take the
    /// service of the others), and strips their edges.
    pub(crate) fn mark_cycle(&mut self, at: PackageId) {
        let mut members = Steps::new();
        for n in self.nodes() {
            if self.depends_on(at, n.id()) && self.depends_on(n.id(), at) {
                members.push(n.id());
            }
        }
        for m in members {
            self.mark_defect(m, Refusal::Cycle { package: m });
        }
    }
}

#[cfg(test)]
mod tests;
