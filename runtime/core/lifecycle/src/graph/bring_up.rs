use super::{PackageGraph, Resolved};
use crate::live::LiveSet;
use crate::refusal::Refusal;
use crate::steps::{sort, Steps};
use crate::PackageId;

impl PackageGraph {
    /// The packages to bring up so that every root is up: services before their consumers; packages already in `live` are skipped. Refuses a root that is
    /// not in the graph, a need that no service meets and a service at an incompatible version.
    pub fn bring_up_order(&self, roots: &[PackageId], live: &LiveSet) -> Result<Steps, Refusal> {
        let mut sorted = Steps::new();
        for &r in roots {
            if self.node(r).is_none() {
                return Err(Refusal::UnknownPackage { package: r });
            }
            if !sorted.contains(r) {
                sorted.push(r);
            }
        }
        sort(&mut sorted);
        let mut out = Steps::new();
        for r in sorted {
            self.visit(r, live, &mut out)?;
        }
        Ok(out)
    }

    /// Depth first: the services of `p`, then `p`. The graph has no cycle, so the recursion ends.
    fn visit(&self, p: PackageId, live: &LiveSet, out: &mut Steps) -> Result<(), Refusal> {
        if live.contains(p) || out.contains(p) {
            return Ok(());
        }
        let node = self.node(p).ok_or(Refusal::UnknownPackage { package: p })?;
        let mut services = Steps::new();
        for need in node.needs() {
            match self.resolve(need) {
                Resolved::Met(id) => {
                    if !services.contains(id) {
                        services.push(id);
                    }
                }
                // a soft need that nothing meets does not stop the load
                Resolved::Incompatible | Resolved::None if !need.is_hard() => {}
                Resolved::Incompatible => {
                    return Err(Refusal::Incompatible {
                        package: p,
                        service: need.service(),
                    })
                }
                Resolved::None => {
                    return Err(Refusal::Unmet {
                        package: p,
                        service: need.service(),
                    })
                }
            }
        }
        sort(&mut services);
        for s in services {
            self.visit(s, live, out)?;
        }
        out.push(p);
        Ok(())
    }
}
