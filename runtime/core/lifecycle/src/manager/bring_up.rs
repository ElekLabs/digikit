use super::refused::RefusedList;
use super::{LifecycleError, PackageManager, Report};
use crate::graph::PackageGraph;
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::state::State;
use crate::steps::{sort, Steps};
use crate::{install::InstallError, PackageId};
use dkrt_abi::provider::{Status, Step as Attach};

fn failed(package: PackageId, entering: State, status: Status) -> LifecycleError {
    LifecycleError::Failed {
        package,
        entering,
        status,
    }
}

/// What a bring-up is for: the packages it was asked for (they are the ones that stay when nothing needs them), whether they get a pin, and whether the first
/// failure ends it.
pub(super) struct Request<'a> {
    pub(super) roots: &'a [PackageId],
    pub(super) pin: bool,
    pub(super) stop_on_failure: bool,
    /// A boot or a reload: the host plans what the services keep once every service has been tried ([`ProviderHost::services_up`]).
    pub(super) numbering: bool,
}

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// Brings up the selection's `roots` and what they need, in graph order, and pins the roots that came up (a selection entry stays up without a consumer).
    /// A package that does not fit or does not come up is refused with the packages that need it ([`Report::refused`]); the rest continues. A root the graph
    /// refuses (an unmet need) is refused alone. Services that a refused package left without a consumer are taken down.
    pub fn boot(&mut self, roots: &[PackageId]) -> Result<Report, LifecycleError> {
        self.refuse_while_pending()?;
        let mut report = Report::default();
        let mut sorted = Steps::new();
        for &r in roots {
            if !sorted.contains(r) {
                sorted.push(r);
            }
        }
        sort(&mut sorted);
        // one root at a time: a root that the graph refuses does not stop the others
        let mut order = Steps::new();
        let mut seen = self.live;
        for r in sorted {
            match self.graph.bring_up_order(&[r], &seen) {
                Ok(steps) => {
                    for p in steps {
                        order.push(p);
                        seen.insert(p);
                    }
                }
                Err(refusal) => report.refused.push(r, LifecycleError::Graph(refusal)),
            }
        }
        // every service before the first module: a module's `init` finds every service of the boot live (the order the boot always had), and a service that
        // fails is known before any module is tried. A service never needs a module, so the graph order holds.
        let order = self.services_first(&order);
        let request = Request {
            roots: sorted.as_slice(),
            pin: true,
            stop_on_failure: false,
            numbering: true,
        };
        self.run_bring_up(&order, &request, &mut report);
        Ok(report)
    }

    /// Brings up `id` and the services it needs that are not up; the same steps as [`PackageManager::boot`]. `id` is pinned: a request names it. The first
    /// package that is refused ends the load, and its reason is the answer.
    pub fn load(&mut self, id: PackageId) -> Result<Report, LifecycleError> {
        self.refuse_while_pending()?;
        if self.live.contains(id) {
            return Err(LifecycleError::AlreadyLive { package: id });
        }
        let order = self
            .graph
            .bring_up_order(&[id], &self.live)
            .map_err(LifecycleError::Graph)?;
        let mut report = Report::default();
        let request = Request {
            roots: &[id],
            pin: true,
            stop_on_failure: true,
            numbering: false,
        };
        self.run_bring_up(&order, &request, &mut report);
        match report.refused().first() {
            Some(refused) => Err(refused.reason),
            None => Ok(report),
        }
    }

    /// `order` with its services first, each group in its own order.
    fn services_first(&self, order: &Steps) -> Steps {
        let is_service = |p: PackageId| self.graph.node(p).is_some_and(|n| n.provides().is_some());
        let mut out = Steps::new();
        for &p in order.as_slice().iter().filter(|&&p| is_service(p)) {
            out.push(p);
        }
        for &p in order.as_slice().iter().filter(|&&p| !is_service(p)) {
            out.push(p);
        }
        out
    }

    /// The services `p` needs in the order they are attached to it: by the host's callback rank, ties by id.
    pub(super) fn attach_order(&self, p: PackageId) -> Steps {
        let mut services = Steps::new();
        for s in self.graph.services_of(p) {
            services.push(s);
        }
        let n = services.len();
        let rank = |s: PackageId| (self.host.attach_rank(s), s.0);
        let list = services.as_mut_slice();
        // insertion sort: at most a few services
        for i in 1..n {
            let mut j = i;
            while j > 0 && rank(list[j - 1]) > rank(list[j]) {
                list.swap(j - 1, j);
                j -= 1;
            }
        }
        services
    }

    pub(super) fn refuse_while_pending(&self) -> Result<(), LifecycleError> {
        match (&self.pending_take_down, self.progress.pending()) {
            (None, None) => Ok(()),
            (work, waiting) => {
                let first = work.and_then(|w| w.todo.as_slice().first().copied());
                Err(LifecycleError::RestorePending {
                    package: waiting.or(first).unwrap_or(PackageId(0)),
                })
            }
        }
    }

    /// Brings up `order` as far as it fits and comes up (see [`PackageManager::boot`]), pins the roots that are up, then settles: what a failure left behind
    /// goes down again.
    pub(super) fn run_bring_up(&mut self, order: &Steps, request: &Request, report: &mut Report) {
        let admitted = self.admit_order(order, &mut report.refused);
        let kept = self.plan_fit(&admitted, request.roots, &[], &mut report.refused);
        let mut started = Steps::new();
        let mut services_done = !request.numbering;
        for (k, &p) in kept.as_slice().iter().enumerate() {
            if !services_done && !self.graph.node(p).is_some_and(|n| n.provides().is_some()) {
                services_done = true;
                self.host.services_up(&self.graph, &self.live);
            }
            if let Some(service) = refused_need(&self.graph, p, &report.refused) {
                report.refused.push(
                    p,
                    LifecycleError::NeedRefused {
                        package: p,
                        service,
                    },
                );
                continue;
            }
            match self.bring_up_package(p) {
                Ok(()) => {
                    report.brought_up.push(p);
                    started.push(p);
                    if self.progress.state(p) == Some(State::Attaching) {
                        report.attaching.push(p);
                    }
                    // the DSP part starts as soon as the package is up and before the next one is placed (the DSP loader holds one reservation at a time); it comes
                    // over UI passes, so this returns at once and never blocks a boot
                    if self.host.start_dsp(p) {
                        report.dsp_pending.push(p);
                    }
                }
                Err(reason) => {
                    report.refused.push(p, reason);
                    if let Some(package) = self.progress.pending() {
                        // the restore of the unwind is busy: nothing else comes up until a retry has finished it
                        for &rest in &kept.as_slice()[k + 1..] {
                            report
                                .refused
                                .push(rest, LifecycleError::RestorePending { package });
                        }
                        break;
                    }
                    if request.stop_on_failure {
                        break;
                    }
                }
            }
        }
        if !services_done {
            self.host.services_up(&self.graph, &self.live);
        }
        if request.pin {
            for &r in request.roots {
                // a root whose attach is pending is pinned too: it is up when the attach ends, and its failure takes the pin off
                if matches!(
                    self.progress.state(r),
                    Some(State::Active | State::Attaching)
                ) {
                    self.graph.set_pinned(r, true);
                }
            }
        }
        self.settle(&started, report);
    }

    /// The end of a bring-up: the services it started that now have no live consumer and no pin go down, and with them (first) a package whose unwind is
    /// waiting for a busy restore. That package is counted live until a retry finishes the take-down.
    fn settle(&mut self, started: &Steps, report: &mut Report) {
        let waiting = self.progress.pending();
        let mut todo = Steps::new();
        if let Some(p) = waiting {
            self.live.insert(p);
            todo.push(p);
        }
        for orphan in self.graph.orphans_among(started, &self.live, &todo) {
            todo.push(orphan);
        }
        if todo.is_empty() {
            return;
        }
        self.pending_take_down = Some(super::PendingTakeDown {
            op: super::pending_take_down::Op::Unload,
            todo,
            rebuild: Steps::new(),
            down: Steps::new(),
            by_manager: false,
        });
        if waiting.is_none() {
            // a busy restore keeps the rest for `retry_pending`
            if let Ok(done) = self.run_pending_take_down() {
                report.taken_down = done.taken_down;
            }
        }
        report.pending = self.progress.pending();
    }

    /// The steps of one package. A failure takes down exactly the steps done (the same code as a take-down, started at the state reached); a restore that
    /// stays busy leaves the package in `RestorePending` (`retry_pending`), and the caller sees the failure that caused the unwind.
    pub(super) fn bring_up_package(&mut self, p: PackageId) -> Result<(), LifecycleError> {
        let at = self
            .placer
            .place(p)
            .map_err(|s| failed(p, State::Placed, s))?;
        self.progress.set(p, State::Placed);
        match self.enter(p, at) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.take_down_package(p, false);
                Err(e)
            }
        }
    }

    fn enter(&mut self, p: PackageId, at: u32) -> Result<(), LifecycleError> {
        let services = self.attach_order(p);
        self.linker
            .link(p, at, services.as_slice())
            .map_err(|s| failed(p, State::Linked, s))?;
        self.progress.set(p, State::Linked);

        let declared = self
            .host
            .start(p)
            .map_err(|s| failed(p, State::Started, s))?;
        self.progress.set(p, State::Started);
        if !self.declaration_matches(p, &declared) {
            return Err(LifecycleError::Declaration { package: p });
        }
        // the new copy takes the state of the old one (or none) before anything is installed or public
        self.host
            .import_state(p)
            .map_err(|s| failed(p, State::Started, s))?;
        // the pools exist before any consumer claims, the package's own claims come after its pools
        self.declare_resources(p)?;
        self.claim_wants(p)?;
        self.progress.set(p, State::Claimed);

        match self.installer.install(p, self.manifest.entries(p)) {
            Ok(()) => self.progress.set(p, State::Installed),
            Err(error) => {
                if error == InstallError::Partial {
                    // some entries stayed: the unwind must restore them
                    self.progress.set(p, State::Installed);
                }
                return Err(LifecycleError::Install { package: p, error });
            }
        }

        self.host
            .activate(p)
            .map_err(|s| failed(p, State::Activated, s))?;
        self.progress.set(p, State::Activated);

        // one call: the host walks the records over all the services (an error leaves nothing attached); the first call of a pending walk is this one, the staged file is
        // valid for it, and `step` makes the others
        let step = self
            .host
            .attach(p, services.as_slice())
            .map_err(|status| failed(p, State::Active, status))?;
        self.progress.set_attached(p, true);
        self.progress.set(
            p,
            if step == Attach::Pending {
                State::Attaching
            } else {
                State::Active
            },
        );
        self.live.insert(p);
        Ok(())
    }
}

/// A service `p` has a hard need of that this operation refused (a soft need never refuses its consumer).
pub(super) fn refused_need(
    graph: &PackageGraph,
    p: PackageId,
    refused: &RefusedList,
) -> Option<PackageId> {
    graph.hard_services_of(p).find(|&s| refused.contains(s))
}
