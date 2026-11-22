use super::bring_up::Request;
use super::pending_take_down::{Op, PendingTakeDown};
use super::{LifecycleError, PackageManager, Report};
use crate::install::InstallError;
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::state::State;
use crate::steps::Steps;
use crate::PackageId;
use dkrt_sdk::E_BUSY;

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// Takes `id` down: its live consumers first, then `id`, then the services left with no consumer and no pin. Every service of that closure is asked
    /// `can_stop` before anything is touched. `id` loses its pin. A restore that answers busy stops the take-down with [`LifecycleError::Busy`]: finish it
    /// with [`PackageManager::retry_pending`]. While it waits, every other operation is refused with [`LifecycleError::RestorePending`].
    pub fn unload(&mut self, id: PackageId) -> Result<Report, LifecycleError> {
        self.refuse_while_pending()?;
        if !self.live.contains(id) {
            return Err(LifecycleError::NotLive { package: id });
        }
        let order = self.graph.take_down_order(id, &self.live);
        self.ask_can_stop(&order)?;
        self.graph.set_pinned(id, false);
        self.pending_take_down = Some(PendingTakeDown {
            op: Op::Unload,
            todo: order,
            rebuild: Steps::new(),
            down: Steps::new(),
        });
        self.run_pending_take_down()
    }

    /// Finishes the pending take-down whatever package is busy: the packages left, in order (for a reload then the bring-up again). The report holds
    /// everything the take-down brought down, including what earlier attempts finished. A restore that is busy or fails again stops it again with the
    /// same error and keeps the rest pending. Refused with [`LifecycleError::NothingPending`] when no take-down waits.
    pub fn retry_pending(&mut self) -> Result<Report, LifecycleError> {
        if self.pending_take_down.is_none() {
            return Err(LifecycleError::NothingPending);
        }
        self.run_pending_take_down()
    }

    /// Asks every service of `order` whether it may stop; the first refusal is the answer.
    pub(super) fn ask_can_stop(&mut self, order: &Steps) -> Result<(), LifecycleError> {
        for &p in order.as_slice() {
            if self.graph.node(p).is_some_and(|n| n.provides().is_some()) {
                self.host
                    .can_stop(p)
                    .map_err(|status| LifecycleError::CannotStop { package: p, status })?;
            }
        }
        Ok(())
    }

    /// Takes down the packages left in the pending work, in order. A busy restore keeps the work for a retry; at the end of a reload the packages come
    /// back up.
    pub(super) fn run_pending_take_down(&mut self) -> Result<Report, LifecycleError> {
        let mut work = self
            .pending_take_down
            .expect("run_pending_take_down needs a pending take-down");
        while let Some(&p) = work.todo.as_slice().first() {
            let done = self.take_down_package(p);
            self.pending_take_down = Some(work);
            done?;
            work.todo.remove(p);
            work.down.push(p);
            self.pending_take_down = Some(work);
        }
        self.pending_take_down = None;
        let mut report = Report {
            taken_down: work.down,
            ..Report::default()
        };
        if work.op == Op::Reload {
            let order = self
                .graph
                .bring_up_order(work.rebuild.as_slice(), &self.live)
                .map_err(LifecycleError::Graph)?;
            let request = Request {
                roots: work.rebuild.as_slice(),
                pin: false,
                stop_on_failure: true,
            };
            self.run_bring_up(&order, &request, &mut report);
            if let Some(refused) = report.refused().first() {
                return Err(refused.reason);
            }
        }
        Ok(report)
    }

    /// Takes one package down from the state it reached: detach from the services it is attached to, release its claims and pools, restore its entries,
    /// stop, unbind, free. A restore that does not finish leaves the package in `RestorePending` (still counted live); a retry starts at the restore.
    pub(super) fn take_down_package(&mut self, p: PackageId) -> Result<(), LifecycleError> {
        let Some(state) = self.progress.state(p) else {
            self.live.remove(p);
            return Ok(());
        };
        let mut services = Steps::new();
        for s in self.graph.services_of(p) {
            services.push(s);
        }
        let attached = self.progress.attached(p);
        for &s in services.as_slice()[..attached].iter().rev() {
            self.host.detach(s, p);
        }
        self.progress.set_attached(p, 0);
        // nothing is public while its entries are restored; a retry of a busy restore has done it
        if state >= State::Activated && state < State::RestorePending {
            self.host.deactivate(p);
        }
        if state >= State::Started {
            self.release_resources(p);
        }
        if state >= State::Installed {
            if let Err(error) = self.installer.restore(p) {
                self.progress.set(p, State::RestorePending);
                return Err(match error {
                    InstallError::Backend(E_BUSY) => LifecycleError::Busy { package: p },
                    error => LifecycleError::RestoreFailed { package: p, error },
                });
            }
        }
        if state >= State::Started {
            self.host.stop(p);
        }
        if state >= State::Linked {
            self.linker.unbind(p);
        }
        self.placer.free(p);
        self.live.remove(p);
        self.progress.remove(p);
        Ok(())
    }
}
