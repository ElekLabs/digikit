use super::PackageGraph;
use crate::live::LiveSet;
use crate::steps::Steps;
use crate::PackageId;

impl PackageGraph {
    /// The packages to take down to unload `target`: its live consumers first, transitively (a consumer before the packages it needs), then `target`, then
    /// each service that the removed packages needed and that is left with no live consumer and no pin, transitively. Empty when `target` is not live.
    pub fn take_down_order(&self, target: PackageId, live: &LiveSet) -> Steps {
        let mut out = Steps::new();
        if !live.contains(target) {
            return out;
        }
        self.consumers_down(target, live, &mut out);
        // orphans: the services of the packages removed so far, then of the orphans themselves
        let mut i = 0;
        while i < out.len() {
            let p = out.as_slice()[i];
            self.orphans_of(p, live, &mut out);
            i += 1;
        }
        out
    }

    fn consumers_down(&self, p: PackageId, live: &LiveSet, out: &mut Steps) {
        if out.contains(p) {
            return;
        }
        for c in self.consumers_of(p) {
            if live.contains(c) {
                self.consumers_down(c, live, out);
            }
        }
        out.push(p);
    }

    /// Adds the unpinned services of `p` that no live package outside `out` still needs, and their own orphaned services.
    fn orphans_of(&self, p: PackageId, live: &LiveSet, out: &mut Steps) {
        for s in self.services_of(p) {
            if out.contains(s) || !live.contains(s) {
                continue;
            }
            let pinned = self.node(s).is_some_and(|n| n.is_pinned());
            if pinned
                || self
                    .consumers_of(s)
                    .any(|c| live.contains(c) && !out.contains(c))
            {
                continue;
            }
            out.push(s);
            self.orphans_of(s, live, out);
        }
    }
}
