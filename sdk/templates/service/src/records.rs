//! `__NAME__` service, handlers (interface B): what the core calls on the service. Every method has a default that does nothing, so the service
//! overrides only what it owns (`dkrt_abi::service::Service`). `validate_records` and `teardown_refusal` are the required methods: say what the service checks about its records and whether a reload must wait.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::{Declare, Problem, Service};

/// The __NAME__ service.
pub struct __CAMEL__;

impl __CAMEL__ {
    /// The rule for the records of kind `kind` of one module (`bodies`: the bodies after the 8-byte header, in file order), as far as the bodies alone decide it: report each
    /// problem, do not stop at the first. `check` runs it at load; `patchkit records --check` runs it on a built module (`module:check`) through the owner map in
    /// packages/patchkit/src/record_rules.rs, which needs one line for a service that owns a kind. A service that owns no record kind, or needs no rule, writes
    /// `dkrt_abi::accepts_records!();` in the `impl` below and has no inherent `validate`. A rule for a kind `KIND_X` looks like this:
    ///
    /// ```ignore
    /// if kind == KIND_X && bodies.len() > 8 {
    ///     report(Problem::set(kind, E_FORMAT, "a module has 8 records at most"));
    /// }
    /// ```
    pub fn validate(_kind: u32, _bodies: &[&[u8]], _report: &mut dyn FnMut(Problem)) {}
}

impl<C: CoreApi> Service<C> for __CAMEL__ {
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        __CAMEL__::validate(kind, bodies, report)
    }

    /// Whether a module of this service may be reloaded now: `0`, or the status that refuses (see `Service::teardown_refusal`). A service with nothing to wait for
    /// writes `dkrt_abi::accepts_teardown!();` instead.
    unsafe fn teardown_refusal(_core: &C, _owner: u8) -> i32 {
        0
    }

    /// The record kinds the service owns: one `d.kind(KIND_X, LEN_X)` for each `[records.kinds]` entry of sdk/sdk.toml with `service = "__NAME__"`
    /// (`patchkit service-check` compares the two).
    unsafe fn declare(_d: &mut Declare) {}
}
