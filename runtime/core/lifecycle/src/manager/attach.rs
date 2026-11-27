use super::pending_take_down::{Op, PendingTakeDown};
use super::{LifecycleError, PackageManager, Report};
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::state::State;
use crate::steps::Steps;
use crate::PackageId;
use dkrt_abi::provider::Step as Attach;

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// One UI pass of what waits (the runtime calls it once per pass):
    ///
    /// - a take-down that the manager started itself (a failed attach) goes on with its busy step;
    /// - each package in `Attaching` is attached again (`ProviderHost::attach`, in package order): `Done` makes it `Active` (the report's `brought_up`), `Pending` keeps it
    ///   (the report's `attaching`), and a refusal, or more passes pending than the manager's `attach_limit`, takes its closure down here (the report's `refused` has the
    ///   package and why, its `taken_down` the packages that went).
    ///
    /// A failed attach waits for a take-down that is pending already (one at a time): the package stays in `Attaching` and the next pass tries again.
    pub fn step(&mut self) -> Report {
        let mut report = Report::default();
        if self.pending_take_down.is_some_and(|w| w.by_manager) {
            self.go_on_with_take_down(&mut report);
        }
        for p in self.progress.attaching() {
            let services = self.attach_order(p);
            match self.host.attach(p, services.as_slice()) {
                Ok(Attach::Done) => {
                    self.progress.set(p, State::Active);
                    report.brought_up.push(p);
                }
                Ok(Attach::Pending) => {
                    if self.progress.wait_one_more(p) > self.attach_limit {
                        self.fail_attach(
                            p,
                            LifecycleError::AttachTimeout { package: p },
                            &mut report,
                        );
                    } else {
                        report.attaching.push(p);
                    }
                }
                Err(status) => {
                    // the host undid its walk (an `Err` leaves nothing attached)
                    self.progress.set_attached(p, false);
                    let reason = LifecycleError::Failed {
                        package: p,
                        entering: State::Active,
                        status,
                    };
                    self.fail_attach(p, reason, &mut report);
                }
            }
        }
        report.pending = self.progress.pending();
        report
    }

    /// Gives up the attach of `p` that is pending (a caller that decided it takes too long, as the live job's timeout): the closure goes down as for a failed attach. `reason` is
    /// what the report says. `NotLive` when `p` is not attaching.
    pub fn cancel_attach(
        &mut self,
        p: PackageId,
        reason: LifecycleError,
    ) -> Result<Report, LifecycleError> {
        if self.progress.state(p) != Some(State::Attaching) {
            return Err(LifecycleError::NotLive { package: p });
        }
        self.refuse_while_pending()?;
        let mut report = Report::default();
        self.fail_attach(p, reason, &mut report);
        Ok(report)
    }

    /// The attach of `p` failed: it loses its pin, its closure goes down (consumers first, no service) and the report says why. A take-down already waiting keeps its place:
    /// `p` stays `Attaching` for the next pass.
    fn fail_attach(&mut self, p: PackageId, reason: LifecycleError, report: &mut Report) {
        if self.pending_take_down.is_some() {
            report.attaching.push(p);
            return;
        }
        report.refused.push(p, reason);
        self.graph.set_pinned(p, false);
        let order = self.graph.reload_order(p, &self.live);
        self.pending_take_down = Some(PendingTakeDown {
            op: Op::Unload,
            todo: order,
            rebuild: Steps::new(),
            down: Steps::new(),
            by_manager: true,
        });
        self.go_on_with_take_down(report);
    }

    /// Runs the pending take-down as far as it goes: what it took down is in the report.
    fn go_on_with_take_down(&mut self, report: &mut Report) {
        if let Ok(done) = self.run_pending_take_down() {
            for &p in done.taken_down.as_slice() {
                report.taken_down.push(p);
            }
        }
    }
}
