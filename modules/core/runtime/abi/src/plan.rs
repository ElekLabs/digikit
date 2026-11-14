//! The verdict of the boot planner (docs/plans/runtime-and-services.md R7), defined once: the planner (`plan.rs` of the runtime) returns one for each
//! module of the selection, and the loader refuses a module before it loads when it is `Refused`. R11 ("boot plan, module states") builds the module
//! states and their UI on this type; it does not add another verdict.
use dkrt_sdk::*;

/// Why the planner refused a module. `code` is the load status the module's result shows (`LOADS`, the boot info).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// A hard need names a service that needs a newer runtime than this one.
    NeedVersion,
    /// A hard need names a service that neither the runtime nor an earlier module of the selection provides.
    NeedMissing,
    /// The header table gives keys for a record kind that no service owns.
    UnownedKind(u32),
    /// Another module of the selection owns the kind, the slot or the hook site the module takes.
    DuplicateOwner(u32),
    /// Another module of the selection has the same key for the kind.
    DuplicateKey(u32, u32),
    /// The module's uses do not fit what the earlier modules left of the resource (a `CAPK_*` kind).
    Fit(u8),
}

impl Reason {
    /// The load status (`[load.status]`).
    pub fn code(&self) -> i32 {
        // if-let chains, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
        if *self == Reason::NeedVersion {
            E_CONTRACT
        } else if *self == Reason::NeedMissing {
            E_MISSING
        } else if let Reason::UnownedKind(_) = *self {
            E_PLAN_KIND
        } else if let Reason::DuplicateOwner(_) = *self {
            E_PLAN_OWNER
        } else if let Reason::DuplicateKey(..) = *self {
            E_PLAN_KEY
        } else {
            E_PLAN_FIT
        }
    }
}

/// The planner's answer for one module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Accepted,
    Refused(Reason),
}

impl Verdict {
    /// `OK` for an accepted module, else the reason's load status.
    pub fn result(&self) -> i32 {
        if let Verdict::Refused(r) = *self {
            r.code()
        } else {
            OK
        }
    }
    pub fn is_accepted(&self) -> bool {
        matches!(self, Verdict::Accepted)
    }
}
