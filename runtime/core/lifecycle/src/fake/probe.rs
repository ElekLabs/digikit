use crate::PackageId;
use dkrt_abi::provider::Status;
use std::cell::RefCell;
use std::rc::Rc;
use std::vec::Vec;

/// One call a seam received. `Attach` and `Detach` carry the service, then the consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    Place(PackageId),
    Link(PackageId),
    Start(PackageId),
    Install(PackageId),
    Attach(PackageId, PackageId),
    CanStop(PackageId),
    Detach(PackageId, PackageId),
    Restore(PackageId),
    Stop(PackageId),
    Unbind(PackageId),
    Free(PackageId),
}

#[derive(Default)]
struct Inner {
    calls: Vec<Call>,
    faults: Vec<(Call, Status)>,
    busy: Vec<(PackageId, usize)>,
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
        self.0.borrow_mut().calls.clear();
    }

    /// The next call equal to `call` fails with `status` (the call is recorded; a failed `Restore` or `CanStop` keeps the package as it is).
    pub fn fail(&self, call: Call, status: Status) {
        self.0.borrow_mut().faults.push((call, status));
    }

    /// The next `times` restores of `package` answer busy.
    pub fn busy_restore(&self, package: PackageId, times: usize) {
        self.0.borrow_mut().busy.push((package, times));
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
