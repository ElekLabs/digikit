//! The payload part of a package's attach and detach walks (`ProviderHost::attach` and `detach` of the runtime's host). The records are applied first (`package_attach`), then each payload
//! of the package's header table goes to the service that owns it (`Payloads::install`, the owner is a hard need, so a row of the package's services). A service may answer
//! `Pending`: the walk is not over and the manager calls `attach` again on a later UI pass.
//!
//! The body of a payload is in the staged file, which is valid only while the package is brought up. So the first `attach` call makes the first `install` call of every payload (with
//! its body, the service copies it then); a later call asks only the payloads that answered `Pending`, with no body. [`PayloadWalk`] is where each package stands.
//!
//! A refusal of a payload undoes the walk: every owner `retire`s for the package and the records are removed (`package_detach`); the answer is `Pending` while a retire is, and the
//! refusal once all are done, so the manager sees an `Err` only when nothing is left attached.
use super::package::Flight;
use super::*;
use dkneeds::keys::MAX_PAYLOADS;
use dkrt_abi::provider::Step;

/// Where the attach walk of one package stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stand {
    /// No attach call yet (or the walk ended).
    Idle,
    /// The records are in and the payloads are being installed: bit `i` is set while payload `i` answered `Pending`.
    Installing { pending: u8 },
    /// A payload refused with the status: the owners retire (`Pending` until all are done), then the refusal is the answer.
    Undoing { status: i32 },
}

/// The walk of every package: one entry each (the manager brings packages up one at a time but lets attaches stay pending, so several can wait).
pub(crate) struct PayloadWalk {
    stand: [Stand; dkrt_abi::REC_MAX],
}

impl PayloadWalk {
    pub(crate) const fn new() -> PayloadWalk {
        PayloadWalk { stand: [Stand::Idle; dkrt_abi::REC_MAX] }
    }

    /// Is the attach of `p` under way (a first call was made and the walk did not end)?
    #[cfg(test)]
    pub(crate) fn is_under_way(&self, p: PackageId) -> bool {
        self.stand[p.0 as usize] != Stand::Idle
    }
}

fn core_ptr() -> *const core::ffi::c_void {
    &crate::package::core_api::REAL_CORE as *const _ as *const core::ffi::c_void
}

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// The payload items of package `id`'s kept header table; `None` when it has none (or no table).
    fn package_payloads(&self, id: PackageId) -> Option<Keys<'_>> {
        let keys = Keys::parse(self.package_table(id)?).ok()?;
        (keys.payload_count() != 0).then_some(keys)
    }

    /// The attach walk of `consumer`: the records, then the payloads (see the module). `flight` holds the staged file of the package that was just brought up; it is read on the
    /// first call only. `OK(Done)` when the walk is over, `Ok(Pending)` while a payload is, the refusal after the undo.
    pub(crate) unsafe fn package_attach_walk(&mut self, consumer: PackageId, services: &[PackageId], flight: &Flight, walk: &mut PayloadWalk) -> Result<Step, i32> {
        let at = consumer.0 as usize;
        if walk.stand[at] == Stand::Idle {
            // a package with no payloads is the walk it always was
            self.package_attach(consumer, services)?;
            if self.package_payloads(consumer).is_none() {
                return Ok(Step::Done);
            }
            walk.stand[at] = self.install_first(consumer, services, flight);
        } else if let Stand::Installing { pending } = walk.stand[at] {
            walk.stand[at] = self.install_again(consumer, services, pending);
        }
        if let Stand::Undoing { status } = walk.stand[at] {
            // the payloads that were installed are given back; the records go with them
            if self.retire_all(consumer, services) == Step::Pending {
                return Ok(Step::Pending);
            }
            self.package_detach(consumer, services);
            walk.stand[at] = Stand::Idle;
            return Err(status);
        }
        match walk.stand[at] {
            Stand::Installing { pending: 0 } => {
                walk.stand[at] = Stand::Idle;
                Ok(Step::Done)
            }
            _ => Ok(Step::Pending),
        }
    }

    /// The first `install` call of every payload of `consumer`, with its body from the staged file.
    unsafe fn install_first(&mut self, consumer: PackageId, services: &[PackageId], flight: &Flight) -> Stand {
        let mut pending = 0u8;
        let mut i = 0;
        while let Some(p) = self.package_payloads(consumer).and_then(|k| k.payload(i)) {
            // the bounds were checked with the file: a body that is not there is a file that changed under the load
            let Some(body) = flight.staged_bytes(p.offset, p.bytes) else {
                return Stand::Undoing { status: E_FORMAT };
            };
            match self.install_one(consumer, services, &p, Some(body)) {
                Ok(Step::Done) => {}
                Ok(Step::Pending) => pending |= 1 << i,
                Err(status) => return Stand::Undoing { status },
            }
            i += 1;
        }
        Stand::Installing { pending }
    }

    /// The later `install` calls: only the payloads that are pending, with no body.
    unsafe fn install_again(&mut self, consumer: PackageId, services: &[PackageId], mut pending: u8) -> Stand {
        let mut i = 0;
        while i < MAX_PAYLOADS {
            if pending & (1 << i) != 0 {
                let Some(p) = self.package_payloads(consumer).and_then(|k| k.payload(i)) else {
                    return Stand::Undoing { status: E_MISSING };
                };
                match self.install_one(consumer, services, &p, None) {
                    Ok(Step::Done) => pending &= !(1 << i),
                    Ok(Step::Pending) => {}
                    Err(status) => return Stand::Undoing { status },
                }
            }
            i += 1;
        }
        Stand::Installing { pending }
    }

    /// One `install` call of payload `p` to its owner among `services`.
    unsafe fn install_one(&mut self, consumer: PackageId, services: &[PackageId], p: &dkneeds::keys::Payload, body: Option<&[u8]>) -> Result<Step, i32> {
        let row = crate::package::provider_rows::walk_rows(services).iter().find(|r| r.name() == p.service).copied();
        match row {
            Some(row) => row.payload_install(core_ptr(), p.kind, body, consumer.0 + 1),
            None => Err(E_MISSING),
        }
    }

    /// `retire` of every owner of `consumer`'s payloads, the last first. `Pending` while one is; every call is repeatable.
    unsafe fn retire_all(&mut self, consumer: PackageId, services: &[PackageId]) -> Step {
        let Some(keys) = self.package_payloads(consumer) else {
            return Step::Done;
        };
        let rows = crate::package::provider_rows::walk_rows(services);
        let mut out = Step::Done;
        // each owner once, whatever the number of its payloads: a retire is for the consumer
        let mut asked = [0u32; MAX_PAYLOADS];
        let mut n = 0;
        let mut i = keys.payload_count();
        while i > 0 {
            i -= 1;
            let Some(p) = keys.payload(i) else { continue };
            if asked[..n].contains(&p.service) || n == MAX_PAYLOADS {
                continue;
            }
            asked[n] = p.service;
            n += 1;
            if let Some(row) = rows.iter().find(|r| r.name() == p.service) {
                if row.payload_retire(core_ptr(), consumer.0 + 1) == Step::Pending {
                    out = Step::Pending;
                }
            }
        }
        out
    }

    /// The detach walk of `consumer`: the owners of its payloads give them back first (`Pending` while one is not done: call again), then the records go (`package_detach`).
    pub(crate) unsafe fn package_detach_walk(&mut self, consumer: PackageId, services: &[PackageId], walk: &mut PayloadWalk) -> Step {
        if self.retire_all(consumer, services) == Step::Pending {
            return Step::Pending;
        }
        walk.stand[consumer.0 as usize] = Stand::Idle;
        self.package_detach(consumer, services);
        Step::Done
    }
}
