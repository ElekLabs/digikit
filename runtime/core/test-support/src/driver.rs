//! What the contract suite drives: the calls the loader makes on one service, over a `FakeCore`. Two implementations: [`OldService`] over an `impl Service<FakeCore>`
//! (the static callbacks of the old trait; deleted with it) and [`ProviderDriver`](crate::ProviderDriver) over an owner object, through the part tables of its row, as the
//! runtime calls them. A service runs the suite with `contract::<S>` or `provider_contract::<R>`; the suite itself is one body.
use crate::FakeCore;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::service::{
    ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, Problem, RowWrite, Service, SettleCtx,
    TableFault,
};
use dkrt_sdk::fault::DIAG_WORDS;

/// The calls of the loader on one service, by the names of the old `Service` trait (the part methods of a provider answer to the same ones).
#[allow(clippy::missing_safety_doc)]
pub trait Driver: Sized {
    /// A fresh service over `core` (the image of the fixture is in it): the instance a provider's `start` makes, nothing for the static callbacks of an old service.
    fn start(core: &FakeCore) -> Self;

    /// The rule for the records of kind `kind`: `Records::validate`, which the part table does not hold.
    fn validate_records(&self, kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem));
    unsafe fn teardown_refusal(&self, core: &FakeCore, owner: u8) -> i32;
    unsafe fn live_done(&self, core: &FakeCore);
    unsafe fn declare(&self, d: &mut Declare);
    unsafe fn check(&self, core: &FakeCore, step: u32, c: &CheckCtx) -> i32;
    /// A consumer came up: the old `apply` over its section.
    unsafe fn apply(&self, core: &FakeCore, c: &ApplyCtx) -> i32;
    /// A consumer without records came up. The old rows were not called for it; a provider's `attach` is.
    unsafe fn attach_without_records(&self, _core: &FakeCore, _owner: u8) {}
    unsafe fn apply_record(&self, core: &FakeCore, kind: u32, body: *const u8, c: &ApplyCtx)
        -> i32;
    unsafe fn remove(&self, core: &FakeCore, owner: u8);
    unsafe fn cap(&self, core: &FakeCore, kind: u8) -> Option<(u32, u32)>;
    unsafe fn attribute(&self, core: &FakeCore, site: u32, id: u32) -> Option<usize>;
    unsafe fn diag(&self, core: &FakeCore, words: &[u32; DIAG_WORDS]);
    unsafe fn table_check(&self, core: &FakeCore, used: u32, out: &mut TableFault) -> u32;
    unsafe fn on_loaded(&self, core: &FakeCore, rec: usize);
    unsafe fn tick(&self, core: &FakeCore, phase: u32);
    unsafe fn module_begin(&self, core: &FakeCore, rec: usize);
    unsafe fn module_commit(&self, core: &FakeCore, rec: usize, c: &ModuleCommitCtx) -> i32;
    unsafe fn settle(&self, core: &FakeCore, s: &SettleCtx);
    unsafe fn dsp_check_sections(&self, core: &FakeCore, sec: &DspSec, job: &mut Job) -> i32;
    unsafe fn dsp_check_rows(&self, core: &FakeCore, sec: &DspSec) -> i32;
    unsafe fn dsp_check_types(&self, core: &FakeCore, sec: &DspSec) -> i32;
    unsafe fn dsp_admit(&self, core: &FakeCore, job: &Job) -> i32;
    unsafe fn dsp_claim(&self, core: &FakeCore, c: &mut Claim);
    unsafe fn dsp_release(&self, core: &FakeCore, rec: usize);
    unsafe fn dsp_clear(&self, core: &FakeCore, rec: usize) -> bool;
    unsafe fn dsp_row_step(
        &self,
        core: &FakeCore,
        rec: usize,
        want: &mut u32,
        write: bool,
    ) -> Option<RowWrite>;
}

/// What a suite case runs over: names the [`Driver`] that calls it. Every `impl Service<FakeCore>` is one (driven through its static callbacks); a provider is run as
/// `Provided<R>` (see `provider_contract`).
pub trait Subject {
    type Driver: Driver;
}

impl<S: Service<FakeCore>> Subject for S {
    type Driver = OldService<S>;
}

/// An old service driven through its static callbacks.
pub struct OldService<S: Service<FakeCore>>(core::marker::PhantomData<S>);

impl<S: Service<FakeCore>> Driver for OldService<S> {
    fn start(_: &FakeCore) -> Self {
        OldService(core::marker::PhantomData)
    }
    fn validate_records(&self, kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        S::validate_records(kind, bodies, report)
    }
    unsafe fn teardown_refusal(&self, core: &FakeCore, owner: u8) -> i32 {
        S::teardown_refusal(core, owner)
    }
    unsafe fn live_done(&self, core: &FakeCore) {
        S::live_done(core)
    }
    unsafe fn declare(&self, d: &mut Declare) {
        S::declare(d)
    }
    unsafe fn check(&self, core: &FakeCore, step: u32, c: &CheckCtx) -> i32 {
        S::check(core, step, c)
    }
    unsafe fn apply(&self, core: &FakeCore, c: &ApplyCtx) -> i32 {
        S::apply(core, c)
    }
    unsafe fn apply_record(
        &self,
        core: &FakeCore,
        kind: u32,
        body: *const u8,
        c: &ApplyCtx,
    ) -> i32 {
        S::apply_record(core, kind, body, c)
    }
    unsafe fn remove(&self, core: &FakeCore, owner: u8) {
        S::remove(core, owner)
    }
    unsafe fn cap(&self, core: &FakeCore, kind: u8) -> Option<(u32, u32)> {
        S::cap(core, kind)
    }
    unsafe fn attribute(&self, core: &FakeCore, site: u32, id: u32) -> Option<usize> {
        S::attribute(core, site, id)
    }
    unsafe fn diag(&self, core: &FakeCore, words: &[u32; DIAG_WORDS]) {
        S::diag(core, words)
    }
    unsafe fn table_check(&self, core: &FakeCore, used: u32, out: &mut TableFault) -> u32 {
        S::table_check(core, used, out)
    }
    unsafe fn on_loaded(&self, core: &FakeCore, rec: usize) {
        S::on_loaded(core, rec)
    }
    unsafe fn tick(&self, core: &FakeCore, phase: u32) {
        S::tick(core, phase)
    }
    unsafe fn module_begin(&self, core: &FakeCore, rec: usize) {
        S::module_begin(core, rec)
    }
    unsafe fn module_commit(&self, core: &FakeCore, rec: usize, c: &ModuleCommitCtx) -> i32 {
        S::module_commit(core, rec, c)
    }
    unsafe fn settle(&self, core: &FakeCore, s: &SettleCtx) {
        S::settle(core, s)
    }
    unsafe fn dsp_check_sections(&self, core: &FakeCore, sec: &DspSec, job: &mut Job) -> i32 {
        S::dsp_check_sections(core, sec, job)
    }
    unsafe fn dsp_check_rows(&self, core: &FakeCore, sec: &DspSec) -> i32 {
        S::dsp_check_rows(core, sec)
    }
    unsafe fn dsp_check_types(&self, core: &FakeCore, sec: &DspSec) -> i32 {
        S::dsp_check_types(core, sec)
    }
    unsafe fn dsp_admit(&self, core: &FakeCore, job: &Job) -> i32 {
        S::dsp_admit(core, job)
    }
    unsafe fn dsp_claim(&self, core: &FakeCore, c: &mut Claim) {
        S::dsp_claim(core, c)
    }
    unsafe fn dsp_release(&self, core: &FakeCore, rec: usize) {
        S::dsp_release(core, rec)
    }
    unsafe fn dsp_clear(&self, core: &FakeCore, rec: usize) -> bool {
        S::dsp_clear(core, rec)
    }
    unsafe fn dsp_row_step(
        &self,
        core: &FakeCore,
        rec: usize,
        want: &mut u32,
        write: bool,
    ) -> Option<RowWrite> {
        S::dsp_row_step(core, rec, want, write)
    }
}
