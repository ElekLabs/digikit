//! What the contract suite drives: the calls the loader makes on one service, over a `FakeCore`. The one implementation is [`ProviderDriver`](crate::ProviderDriver): an owner
//! object, through the part tables of its row, as the runtime calls them. A service runs the suite with `provider_contract::<R>`; the suite itself is one body.
use crate::FakeCore;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::service::{
    ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, Problem, RowWrite, SettleCtx, TableFault,
};
use dkrt_sdk::fault::DIAG_WORDS;

/// The calls of the loader on one service (the part methods of a provider answer to them).
#[allow(clippy::missing_safety_doc)]
pub trait Driver: Sized {
    /// A fresh service over `core` (the image of the fixture is in it): the instance a provider's `start` makes.
    fn start(core: &FakeCore) -> Self;

    /// The address range of the owner object the driver started (`None`: no object, or the driver cannot tell). A snapshot compares a memory word that holds a pointer into it by its
    /// offset (`OWNER_PTR_TAG`), because the owner of a second driver is at another address and a service whose firmware rows point at strings of its owner would differ for that reason only.
    fn owner_range(&self) -> Option<(usize, usize)> {
        None
    }

    /// The rule for the records of kind `kind`: `Records::validate`, which the part table does not hold.
    fn validate_records(&self, kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem));
    unsafe fn teardown_refusal(&self, core: &FakeCore, owner: u8) -> i32;
    unsafe fn live_done(&self, core: &FakeCore);
    unsafe fn declare(&self, d: &mut Declare);
    unsafe fn check(&self, core: &FakeCore, step: u32, c: &CheckCtx) -> i32;
    /// A consumer came up: `attach` over its section.
    unsafe fn apply(&self, core: &FakeCore, c: &ApplyCtx) -> i32;
    /// A consumer without records came up: a provider's `attach` is called for it.
    unsafe fn attach_without_records(&self, core: &FakeCore, owner: u8);
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

/// What a suite case runs over: names the [`Driver`] that calls it. A provider is run as `Provided<R>` (see `provider_contract`).
pub trait Subject {
    type Driver: Driver;
}
