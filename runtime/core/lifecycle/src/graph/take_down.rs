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
        self.consumers_down(target, live, &mut Steps::new(), &mut out);
        // orphans: the services of the packages removed so far, then of the orphans themselves
        let mut i = 0;
        while i < out.len() {
            let p = out.as_slice()[i];
            self.orphans_of(p, live, &mut out);
            i += 1;
        }
        out
    }

    /// The packages to take down and bring back to reload `target`: `target` and its live consumers, transitively, consumers first. The services it
    /// needs stay up, so no orphan is part of a reload. Empty when `target` is not live.
    pub fn reload_order(&self, target: PackageId, live: &LiveSet) -> Steps {
        let mut out = Steps::new();
        if live.contains(target) {
            self.consumers_down(target, live, &mut Steps::new(), &mut out);
        }
        out
    }

    /// `path` holds the packages being visited: a loop of soft needs ends there.
    fn consumers_down(&self, p: PackageId, live: &LiveSet, path: &mut Steps, out: &mut Steps) {
        if out.contains(p) || path.contains(p) {
            return;
        }
        path.push(p);
        for c in self.consumers_of(p) {
            if live.contains(c) {
                self.consumers_down(c, live, path, out);
            }
        }
        path.pop();
        out.push(p);
    }

    /// Adds the unpinned services of `p` that no live package outside `out` still needs, and their own orphaned services.
    fn orphans_of(&self, p: PackageId, live: &LiveSet, out: &mut Steps) {
        for s in self.dependencies_of(p) {
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

    /// Of `candidates` (the packages a bring-up started, in bring-up order) those to take down because nothing needs them: live, unpinned and needed by no
    /// live package outside `removed` and the result, so a service goes together with the last consumer that was refused. In take-down order (consumers first).
    pub fn orphans_among(&self, candidates: &Steps, live: &LiveSet, removed: &Steps) -> Steps {
        let mut gone = *removed;
        let mut out = Steps::new();
        for &p in candidates.as_slice().iter().rev() {
            let pinned = self.node(p).is_some_and(|n| n.is_pinned());
            if gone.contains(p) || !live.contains(p) || pinned {
                continue;
            }
            if self
                .consumers_of(p)
                .any(|c| live.contains(c) && !gone.contains(c))
            {
                continue;
            }
            gone.push(p);
            out.push(p);
        }
        out
    }
}
