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
            self.visit(r, live, &mut Steps::new(), &mut out)?;
        }
        Ok(out)
    }

    /// Depth first: the services of `p`, then `p`. `path` holds the packages being visited: hard edges and kind owners have no cycle, a soft edge to a
    /// package on the path would close one and is skipped, so the recursion ends.
    fn visit(
        &self,
        p: PackageId,
        live: &LiveSet,
        path: &mut Steps,
        out: &mut Steps,
    ) -> Result<(), Refusal> {
        if live.contains(p) || out.contains(p) {
            return Ok(());
        }
        let node = self.node(p).ok_or(Refusal::UnknownPackage { package: p })?;
        if let Some(defect) = node.defect() {
            return Err(defect);
        }
        // a soft provider or an owner that cannot come up is skipped, never a reason to refuse
        let mut services = Steps::new();
        let mut hard = Steps::new();
        for need in node.needs() {
            match self.resolve(need) {
                Resolved::Met(id) => {
                    if !services.contains(id) {
                        services.push(id);
                    }
                    if need.is_hard() && !hard.contains(id) {
                        hard.push(id);
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
        // the owners of the kinds it emits keys for come first too; one that cannot come up itself does not refuse this package (the rules of the core do)
        for &o in node.kind_owners() {
            if !services.contains(o) {
                services.push(o);
            }
        }
        sort(&mut services);
        path.push(p);
        for s in services {
            let must = hard.contains(s);
            if path.contains(s) || (!must && self.depends_on(s, p)) {
                continue;
            }
            let mark = *out;
            if let Err(e) = self.visit(s, live, path, out) {
                // a hard provider that fails refuses `p` with its own error; anything else is skipped
                if must {
                    path.pop();
                    return Err(e);
                }
                *out = mark;
            }
        }
        path.pop();
        out.push(p);
        Ok(())
    }
}
