use crate::PackageId;
use dkrt_abi::provider::Status;
use std::cell::RefCell;
use std::rc::Rc;
use std::vec::Vec;

/// One call a seam received. `Attach` and `Detach` carry the consumer; the services of the call are in [`Probe::walk_of`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// `Manifest::admit`: only for `Probe::fail`; the asks are in [`Probe::admitted`].
    Admit(PackageId),
    /// `ProviderHost::services_up`.
    ServicesUp,
    Place(PackageId),
    Link(PackageId),
    Start(PackageId),
    Install(PackageId),
    Activate(PackageId),
    Deactivate(PackageId),
    Attach(PackageId),
    CanStop(PackageId),
    Detach(PackageId),
    Restore(PackageId),
    Stop(PackageId),
    /// `ProviderHost::export_state`.
    ExportState(PackageId),
    /// `ProviderHost::import_state`.
    ImportState(PackageId),
    StartDsp(PackageId),
    Unbind(PackageId),
    Free(PackageId),
}

#[derive(Default)]
struct Inner {
    calls: Vec<Call>,
    faults: Vec<(Call, Status)>,
    busy: Vec<(PackageId, usize)>,
    walks: Vec<(PackageId, Vec<PackageId>)>,
    admits: Vec<PackageId>,
}

/// The call log and the faults of one fake world. Clones share the same state.
#[derive(Clone, Default)]
pub struct Probe(Rc<RefCell<Inner>>);

impl Probe {
    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.0.borrow().calls.clone()
    }

    pub fn clear(&self) {
        let mut i = self.0.borrow_mut();
        i.calls.clear();
        i.admits.clear();
    }

    /// The packages `Manifest::admit` was asked about, in order. They are not in [`Probe::calls`]: the manifest is not a seam of the steps of a package.
    pub fn admitted(&self) -> Vec<PackageId> {
        self.0.borrow().admits.clone()
    }

    /// Records an `admit` of `package`; the status of a fault set with `fail(Call::Admit(package), ..)`, once.
    pub(super) fn record_admit(&self, package: PackageId) -> Result<(), Status> {
        let mut i = self.0.borrow_mut();
        i.admits.push(package);
        let call = Call::Admit(package);
        match i.faults.iter().position(|(c, _)| *c == call) {
            Some(k) => Err(i.faults.remove(k).1),
            None => Ok(()),
        }
    }

    /// The next call equal to `call` fails with `status` (the call is recorded; a failed `Restore` or `CanStop` keeps the package as it is).
    pub fn fail(&self, call: Call, status: Status) {
        self.0.borrow_mut().faults.push((call, status));
    }

    /// The next `times` restores of `package` answer busy.
    pub fn busy_restore(&self, package: PackageId, times: usize) {
        self.0.borrow_mut().busy.push((package, times));
    }

    /// The services of the last `attach` or `detach` of `consumer`, in the order the call gave them.
    pub fn walk_of(&self, consumer: PackageId) -> Vec<PackageId> {
        let i = self.0.borrow();
        i.walks
            .iter()
            .rev()
            .find(|(c, _)| *c == consumer)
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    pub(super) fn walk(&self, consumer: PackageId, services: &[PackageId]) {
        self.0
            .borrow_mut()
            .walks
            .push((consumer, services.to_vec()));
    }

    /// Records `call`; the status of a matching fault, once.
    pub(super) fn record(&self, call: Call) -> Result<(), Status> {
        let mut i = self.0.borrow_mut();
        i.calls.push(call);
        match i.faults.iter().position(|(c, _)| *c == call) {
            Some(k) => Err(i.faults.remove(k).1),
            None => Ok(()),
        }
    }

    /// Whether a restore of `package` answers busy now (counts one down).
    pub(super) fn take_busy(&self, package: PackageId) -> bool {
        let mut i = self.0.borrow_mut();
        match i.busy.iter_mut().find(|(p, n)| *p == package && *n > 0) {
            Some((_, n)) => {
                *n -= 1;
                true
            }
            None => false,
        }
    }
}
