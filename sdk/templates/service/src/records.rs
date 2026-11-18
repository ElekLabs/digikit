//! `__NAME__` service, handlers (interface B): what the core calls on the service. Every method has a default that does nothing, so the service
//! overrides only what it owns (`dkrt_abi::service::Service`). `validate_records` is the one required method: say what the service checks about its records.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::{Declare, Problem, Service};

/// The __NAME__ service.
pub struct __CAMEL__;

impl<C: CoreApi> Service<C> for __CAMEL__ {
    /// The rule for the records of kind `kind` of one module, as a set (`bodies`: the bodies after the 8-byte header, in file order). `check` runs it at load and
    /// `patchkit records --check` runs it on a built module (`module:check`), so write the rule here once. A service that owns no record kind, or needs no rule, writes
    /// `dkrt_abi::accepts_records!();` instead. A rule for a kind `KIND_X` looks like this (report each problem, do not stop at the first):
    ///
    /// ```ignore
    /// if kind == KIND_X && bodies.len() > 8 {
    ///     report(Problem::set(kind, E_FORMAT, "a module has 8 records at most"));
    /// }
    /// ```
    fn validate_records<R: FnMut(Problem)>(_kind: u32, _bodies: &[&[u8]], _report: &mut R) {}

    /// The record kinds the service owns: one `d.kind(KIND_X, LEN_X)` for each `[records.kinds]` entry of sdk/sdk.toml with `service = "__NAME__"`
    /// (`patchkit service-check` compares the two).
    unsafe fn declare(_d: &mut Declare) {}
}
