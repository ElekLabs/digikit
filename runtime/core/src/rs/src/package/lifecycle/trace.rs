//! The runtime's lifecycle events (docs/INSTRUMENTATION.md "Lifecycle events"): [`RtTrace`] is the [`Trace`] of the manager's seams over the event log (`LIFECYCLE`), and the
//! calls of the live job and the installer's gate add the events only they can see.
//!
//! An event names the package by the FNV-1a hash of its file name without the extension (a record number is reused by the next package that takes the place, a hash is
//! not) and the operation by a number that counts up for each boot and each live job. Events are sent only while a log sink listens (`dlog::log_on`): they are never kept
//! in the small ring, so a boot's own events stay in it. The producer pays one test while nothing listens.

use super::session::Handle;
use crate::package::resident::dlog;
use crate::package::loader::{Images, Registry};
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::package::PackageId;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::MAX_MODULES;
use super::{RtHost, RtInstallers, RtLinker, RtPlacer, Session};
use dkrt_lifecycle::{Phase, Step, Trace, Traced};
use dkrt_sdk::logk;

/// The operation now running (a boot, or the live job): 0 before the first.
static mut OP: u32 = 0;

/// A boot or a live job starts: its events carry a new operation number, which the caller keeps for the events it emits itself (`emit`). The manager's seams (`RtTrace`)
/// read the number of the operation now running (`current_op`).
pub(crate) fn next_op() -> u32 {
    unsafe {
        let op = read_volatile(addr_of!(OP)).wrapping_add(1);
        write_volatile(addr_of_mut!(OP), op);
        op
    }
}

/// The operation now running, for the events of the manager's seams.
pub(crate) fn current_op() -> u32 {
    unsafe { read_volatile(addr_of!(OP)) }
}

/// FNV-1a 32 of the file name in the NUL terminated `path`, without its directory and its extension (0 for no path).
pub(crate) unsafe fn stem_hash(path: *const u8) -> u32 {
    if path.is_null() {
        return 0;
    }
    let n = dkrt_abi::mem::cstr_len(path, 128);
    let name = core::slice::from_raw_parts(path, n);
    let from = name.iter().rposition(|b| *b == b'/').map_or(0, |i| i + 1);
    let name = &name[from..];
    let to = name.iter().rposition(|b| *b == b'.').unwrap_or(name.len());
    dkneeds::keys::name_hash_bytes(&name[..to])
}

/// Whether anything listens (the one test of a producer).
pub(crate) fn on() -> bool {
    dlog::log_on()
}

/// The package of an event that belongs to none (a load whose file is in no record yet, a park wait): kind and record are those of no real package.
pub(crate) const NO_PACKAGE: PackageId = PackageId(0xff);

/// The kind word of an event: a record below `MAX_MODULES` is a module, above it a service.
fn kind_of(package: PackageId) -> u32 {
    (package.0 as usize >= MAX_MODULES) as u32
}

/// Log one event of operation `op`. `detail` is the fifth word: what the step adds (the task a park wait waits for), else 0.
pub(crate) unsafe fn emit(op: u32, phase: Phase, step: Step, package: PackageId, name_hash: u32, status: i32, detail: u32) {
    let info = phase as u32 | (step as u32) << 8 | kind_of(package) << 16 | (package.0 as u32) << 24;
    dlog::ev(logk::LIFECYCLE, dlog::INFO, [op, name_hash, info, status as u32, detail]);
}

/// Log what the state handoff did for the service named `name_hash` (`STATE_CARRIED`): nothing for a service that started fresh.
pub(crate) unsafe fn emit_state(name_hash: u32, outcome: &super::carried_state::Outcome) {
    use super::carried_state::Outcome;
    let (code, version, len) = match *outcome {
        Outcome::Fresh => return,
        Outcome::Carried { version, len } => (0, version, len),
        Outcome::Dropped { why, version, len } => (why as u32, version, len),
    };
    dlog::ev(logk::STATE_CARRIED, dlog::INFO, [current_op(), name_hash, code, version as u32, len]);
}

/// The trace of one manager's seams: it finds the file name of a package through the session the seams share.
pub(crate) struct RtTrace<P: PackagePlatform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: PackagePlatform, M: Images, R: Registry> RtTrace<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtTrace(h)
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> Clone for RtTrace<P, M, R> {
    fn clone(&self) -> Self {
        RtTrace(self.0)
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> Trace for RtTrace<P, M, R> {
    fn on(&self) -> bool {
        on()
    }

    type Name = u32;

    fn name_of(&self, package: PackageId) -> u32 {
        unsafe {
            let (s, _) = self.0.get();
            let mut buf = [0u8; crate::package::loader::discover::PATH_CAP + 1];
            stem_hash(s.path_of(package, &mut buf).unwrap_or(core::ptr::null()))
        }
    }

    fn event(&self, phase: Phase, step: Step, package: PackageId, name: u32, status: i32) {
        unsafe {
            // the end of a place says where the image went, so a spinning program counter of a device run maps to its package (the size is in the module list)
            let detail = if phase == Phase::End && step == Step::Place && status == 0 { self.0.get().0.flight.placed_span().0 } else { 0 };
            emit(current_op(), phase, step, package, name, status, detail)
        }
    }
}

/// The seams of one manager over `session`, each traced to the log (`Session::seams`, wrapped): the one place the runtime builds the manager's seams.
pub(crate) fn traced_seams<P: PackagePlatform, M: Images, R: Registry>(
    session: &mut Session<P, M, R>,
) -> (
    Traced<RtPlacer<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtLinker<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtInstallers<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtHost<P, M, R>, RtTrace<P, M, R>>,
) {
    let trace = RtTrace::new(Handle(session as *mut _));
    let (placer, linker, installers, host) = session.seams();
    (Traced::new(placer, trace.clone()), Traced::new(linker, trace.clone()), Traced::new(installers, trace.clone()), Traced::new(host, trace))
}
