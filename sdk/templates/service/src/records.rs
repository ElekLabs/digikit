//! `__NAME__` service, the owner object and its parts (interface B): what the core calls on the service. The lifecycle (`Provider`) is required (`start` makes the object);
//! every other part is a separate trait the service adopts by name in the `service!` call of `dks/src/lib.rs` (`Records`, `DspPart`, `Ticker`, `Query`, `Describe`; see
//! `dkrt_abi::provider`). A method of a part has a default that does nothing, so the service overrides only what it owns.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{Provider, Records, Status};
use dkrt_abi::service::Problem;

/// The __NAME__ service: the one object the file's cell holds. Its state is private and changes through its methods.
pub struct __CAMEL__;

impl __CAMEL__ {
    /// The rule for the records of kind `kind` of one module (`bodies`: the bodies after the 8-byte header, in file order), as far as the bodies alone decide it: report each
    /// problem, do not stop at the first. `Records::validate` runs it, so the load (`check`) and `patchkit records --check` on a built module (`module:check`, through the owner map
    /// in packages/patchkit/src/record_rules.rs, which needs one line for a service that owns a kind) cannot differ on these rules. A rule for a kind `KIND_X` looks like this:
    ///
    /// ```ignore
    /// if kind == KIND_X && bodies.len() > 8 {
    ///     report(Problem::set(kind, E_FORMAT, "a module has 8 records at most"));
    /// }
    /// ```
    pub fn validate(_kind: u32, _bodies: &[&[u8]], _report: &mut dyn FnMut(Problem)) {}
}

impl<C: CoreApi> Provider<C> for __CAMEL__ {
    /// The file's `init`, before the declarations: make the object. No consumer is attached yet.
    fn start_in(_core: &C, slot: &mut core::mem::MaybeUninit<__CAMEL__>) -> Result<(), Status> {
        slot.write(__CAMEL__);
        Ok(())
    }
}

impl<C: CoreApi> Records<C> for __CAMEL__ {
    fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        __CAMEL__::validate(kind, bodies, report)
    }
}
