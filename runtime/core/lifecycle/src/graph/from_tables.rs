use super::PackageGraph;
use crate::node::{Api, Need, Node};
use crate::refusal::Refusal;
use crate::{PackageId, MAX_PACKAGES};
use dkneeds::keys::{tag, Keys};
use dkrt_abi::version::version_satisfies;

impl PackageGraph {
    /// The graph of the packages whose header tables are given (`dkneeds` reads them: a service's `PROVIDES` is its API, the `NEEDS` are the needs).
    ///
    /// `core_apis` are the APIs the core and the image provide without a package (the `runtime` floor and the services built in). A need on one of them
    /// leaves no edge: a hard need at a version the API does not reach is [`Refusal::Incompatible`] here, a soft one is dropped. Every other need is
    /// resolved against the services of the graph, so a hard need that nothing meets is refused later by [`PackageGraph::bring_up_order`].
    ///
    /// A table that does not parse, or a package with more than one `PROVIDES` item, is [`Refusal::BadTable`]; the other refusals are those of
    /// [`PackageGraph::from_nodes`].
    pub fn from_tables(
        packages: &[(PackageId, &[u8])],
        core_apis: &[Api],
    ) -> Result<PackageGraph, Refusal> {
        if packages.len() > MAX_PACKAGES {
            return Err(Refusal::TooManyPackages);
        }
        let mut nodes = [Node::new(PackageId(0)); MAX_PACKAGES];
        for (i, (id, bytes)) in packages.iter().enumerate() {
            nodes[i] = node_of(*id, bytes, core_apis)?;
        }
        PackageGraph::from_nodes(&nodes[..packages.len()])
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
        let core = core_apis.iter().find(|a| a.name() == need.service);
        match core {
            Some(a) if version_satisfies(a.version(), need.min_version) => continue,
            Some(_) if need.hard => {
                return Err(Refusal::Incompatible {
                    package: id,
                    service: need.service,
                })
            }
            Some(_) => continue,
            None => {}
        }
        node = node.needing(if need.hard {
            Need::new(need.service, need.min_version)
        } else {
            Need::soft(need.service, need.min_version)
        });
    }
    Ok(node)
}

#[cfg(test)]
mod tests;
