use super::pending_take_down::{Op, PendingTakeDown};
use super::refused::RefusedList;
use super::resources::owner_id;
use super::{LifecycleError, PackageManager, Report};
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::steps::Steps;
use crate::{PackageId, MAX_PACKAGES};
use dkrt_fit::ledger::OwnerId;

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// Takes `id` and its live consumers down (consumers first) and brings them up again (services first): reloading a service reloads its consumers.
    /// The services `id` needs stay up, so a reload touches no orphan, and pins stay as they were. Every service in the set is asked `can_stop` first.
    /// A restore that answers busy stops it with [`LifecycleError::Busy`]; finish it with [`PackageManager::retry_pending`].
    pub fn reload(&mut self, id: PackageId) -> Result<Report, LifecycleError> {
        self.refuse_while_pending()?;
        if !self.live.contains(id) {
            return Err(LifecycleError::NotLive { package: id });
        }
        let order = self.graph.reload_order(id, &self.live);
        self.check_reload_fit(&order)?;
        self.ask_can_stop(&order)?;
        self.pending_take_down = Some(PendingTakeDown {
            op: Op::Reload,
            todo: order,
            rebuild: order,
            down: Steps::new(),
        });
        self.run_pending_take_down()
    }

    /// Whether the closure fits when it comes back: its new pools and claims against the ledger with the closure's own pools and claims returned. Nothing
    /// is touched.
    fn check_reload_fit(&self, closure: &Steps) -> Result<(), LifecycleError> {
        let mut after = self.live;
        let mut owners = [OwnerId(0); MAX_PACKAGES];
        for (i, &p) in closure.as_slice().iter().enumerate() {
            after.remove(p);
            owners[i] = owner_id(p);
        }
        let order = self
            .graph
            .bring_up_order(closure.as_slice(), &after)
            .map_err(LifecycleError::Graph)?;
        let mut refused = RefusedList::new();
        self.plan_fit(
            &order,
            closure.as_slice(),
            &owners[..closure.len()],
            &mut refused,
        );
        match refused.as_slice().first() {
            Some(first) => Err(first.reason),
            None => Ok(()),
        }
    }
}
