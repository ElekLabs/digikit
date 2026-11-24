//! The provider side of the contract suite: an owner object driven through the part tables of its `ProviderRow`, as the runtime calls them (`ProviderHandle`), over a
//! `FakeCore`. `RowBuilder` makes the row the way `service!` does (the parts a service adopts, each table from `of::<P, FakeCore>()`), and [`ProviderDriver`] is the
//! [`Driver`] of the suite over it. The old static-callback suite and this one are one body (`contract::run`).
use crate::contract::{run, Fixture};
use crate::driver::{Driver, Subject};
use crate::subject::SubjectGuard;
use crate::FakeCore;
use core::ffi::c_void;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{
    Describe, DescribeV1, DspPart, DspV1, LifecycleV1, PartRef, Provider, ProviderHandle,
    ProviderRow, Query, QueryV1, Records, RecordsV1, Status, TickV1, Ticker, DESCRIBE_NEWEST,
    DSP_NEWEST, LIFECYCLE_NEWEST, PART_DESCRIBE, PART_DSP, PART_LIFECYCLE, PART_QUERY,
    PART_RECORDS, PART_TICK, QUERY_NEWEST, RECORDS_NEWEST, TICK_NEWEST,
};
use dkrt_abi::service::{
    ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, Problem, RowWrite, SettleCtx, TableFault,
};
use dkrt_sdk::fault::DIAG_WORDS;
use std::boxed::Box;
use std::vec::Vec;

type Validate = fn(u32, &[&[u8]], &mut dyn FnMut(Problem));

fn accepts(_: u32, _: &[&[u8]], _: &mut dyn FnMut(Problem)) {}

fn leak<T>(table: T) -> *const () {
    Box::leak(Box::new(table)) as *const T as *const ()
}

/// The row of an owner object `P` over `FakeCore`, built like the `service!` macro builds it: the lifecycle part and the parts `P` adopts.
pub struct RowBuilder<P> {
    name: &'static str,
    kinds: &'static [(u32, u32)],
    parts: Vec<PartRef>,
    validate: Validate,
    stop: unsafe fn(*mut c_void, &FakeCore),
    _p: core::marker::PhantomData<P>,
}

unsafe fn stop_of<P: Provider<FakeCore>>(instance: *mut c_void, core: &FakeCore) {
    let mut boxed = Box::from_raw(instance as *mut P);
    P::stop(&mut boxed, core)
}

impl<P: Provider<FakeCore> + 'static> RowBuilder<P> {
    /// A row for the service `name` with the lifecycle part.
    pub fn new(name: &'static str) -> RowBuilder<P> {
        RowBuilder {
            name,
            kinds: &[],
            parts: std::vec![PartRef::new(
                PART_LIFECYCLE,
                LIFECYCLE_NEWEST,
                leak(LifecycleV1::of::<P, FakeCore>())
            )],
            validate: accepts,
            stop: stop_of::<P>,
            _p: core::marker::PhantomData,
        }
    }

    /// Adopt the records part for the kinds `kinds` (kind, whole record length).
    pub fn records(mut self, kinds: &'static [(u32, u32)]) -> Self
    where
        P: Records<FakeCore>,
    {
        self.kinds = kinds;
        self.validate = <P as Records<FakeCore>>::validate;
        self.parts.push(PartRef::new(
            PART_RECORDS,
            RECORDS_NEWEST,
            leak(RecordsV1::of::<P, FakeCore>(kinds)),
        ));
        self
    }

    pub fn dsp(mut self) -> Self
    where
        P: DspPart<FakeCore>,
    {
        self.parts.push(PartRef::new(
            PART_DSP,
            DSP_NEWEST,
            leak(DspV1::of::<P, FakeCore>()),
        ));
        self
    }

    pub fn tick(mut self) -> Self
    where
        P: Ticker<FakeCore>,
    {
        self.parts.push(PartRef::new(
            PART_TICK,
            TICK_NEWEST,
            leak(TickV1::of::<P, FakeCore>()),
        ));
        self
    }

    pub fn query(mut self) -> Self
    where
        P: Query,
    {
        self.parts.push(PartRef::new(
            PART_QUERY,
            QUERY_NEWEST,
            leak(QueryV1::of::<P>()),
        ));
        self
    }

    pub fn describe(mut self) -> Self
    where
        P: Describe<FakeCore>,
    {
        self.parts.push(PartRef::new(
            PART_DESCRIBE,
            DESCRIBE_NEWEST,
            leak(DescribeV1::of::<P, FakeCore>()),
        ));
        self
    }

    /// Start the owner object over `core` (`Provider::start`) and resolve the row for it, as the runtime does after `dk_provider_declare_row`.
    pub fn start(self, core: &FakeCore) -> Result<Started, Status> {
        let instance = Box::into_raw(Box::new(P::start(core)?)) as *mut c_void;
        let parts: &'static [PartRef] = Box::leak(self.parts.into_boxed_slice());
        let row: &'static ProviderRow = Box::leak(Box::new(ProviderRow::new(
            dkrt_abi::provider::name_hash(self.name),
            parts,
        )));
        // SAFETY: the row, its parts and tables are leaked boxes; the instance is live until `stop`.
        let handle = unsafe { ProviderHandle::resolve(row, 0..usize::MAX, instance) };
        match handle {
            Ok(handle) => Ok(Started {
                handle,
                validate: self.validate,
                stop: self.stop,
            }),
            Err(e) => {
                // SAFETY: the instance was made above and nothing else holds it.
                unsafe { (self.stop)(instance, core) };
                Err(e)
            }
        }
    }
}

/// A started owner object and its resolved row.
pub struct Started {
    handle: ProviderHandle,
    validate: Validate,
    stop: unsafe fn(*mut c_void, &FakeCore),
}

/// How a service says which parts it adopts: its owner object and the row of `RowBuilder`.
pub trait Recipe {
    type P: Provider<FakeCore> + 'static;
    /// The name of the service.
    const NAME: &'static str;
    fn parts(b: RowBuilder<Self::P>) -> RowBuilder<Self::P>;
}

/// The cases of the contract for the provider `R` describes, on the profile of `f`.
pub fn provider_contract<R: Recipe>(f: &Fixture) {
    run::<Provided<R>>(f)
}

/// A provider as the subject of a suite case: `Lifecycle::<Provided<R>>`.
pub struct Provided<R: Recipe>(core::marker::PhantomData<R>);

impl<R: Recipe> Subject for Provided<R> {
    type Driver = ProviderDriver<R>;
}

/// The suite's [`Driver`] over a started provider: every call goes through the part tables of its row.
pub struct ProviderDriver<R: Recipe> {
    started: Started,
    /// The instance as the `subject` of the fixture's functions, while the driver lives.
    _subject: SubjectGuard,
    _r: core::marker::PhantomData<R>,
}

fn cptr(core: &FakeCore) -> *const c_void {
    core as *const FakeCore as *const c_void
}

impl<R: Recipe> ProviderDriver<R> {
    /// The handle the runtime would keep for the provider.
    pub fn handle(&self) -> &ProviderHandle {
        &self.started.handle
    }

    /// The owner object, for a test that reads what a part function changed.
    pub fn with_instance<T>(&self, f: impl FnOnce(&mut R::P) -> T) -> T {
        // SAFETY: the instance is the `R::P` that `start` made, live until `stop`; a test runs one call at a time.
        f(unsafe { &mut *(self.started.handle.instance() as *mut R::P) })
    }

    /// `Provider::stop` (the file's `fini`): the instance is gone after.
    pub fn stop(self, core: &FakeCore) {
        // SAFETY: the instance is the one `start` made; `self` is consumed, so nothing calls it again.
        unsafe { (self.started.stop)(self.started.handle.instance(), core) }
    }
}

impl<R: Recipe> Driver for ProviderDriver<R> {
    fn start(core: &FakeCore) -> Self {
        let started = R::parts(RowBuilder::<R::P>::new(R::NAME))
            .start(core)
            .unwrap_or_else(|e| panic!("{}: start refused with {e}", R::NAME));
        let _subject = SubjectGuard::publish(started.handle.instance() as *mut R::P);
        ProviderDriver {
            started,
            _subject,
            _r: core::marker::PhantomData,
        }
    }
    fn validate_records(&self, kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        (self.started.validate)(kind, bodies, report)
    }
    unsafe fn teardown_refusal(&self, core: &FakeCore, _owner: u8) -> i32 {
        self.handle().can_stop(cptr(core))
    }
    unsafe fn live_done(&self, core: &FakeCore) {
        self.handle().reload_done(cptr(core))
    }
    unsafe fn declare(&self, d: &mut Declare) {
        for &(kind, len) in self.handle().kinds() {
            d.kind(kind, len);
        }
    }
    unsafe fn check(&self, core: &FakeCore, step: u32, c: &CheckCtx) -> i32 {
        self.handle().check(cptr(core), step, c)
    }
    unsafe fn apply(&self, core: &FakeCore, c: &ApplyCtx) -> i32 {
        self.handle().attach(cptr(core), c)
    }
    unsafe fn attach_without_records(&self, core: &FakeCore, owner: u8) {
        let c = ApplyCtx {
            owner,
            reg: core::ptr::null(),
            base: 0,
        };
        self.handle().attach(cptr(core), &c);
    }
    unsafe fn apply_record(
        &self,
        core: &FakeCore,
        kind: u32,
        body: *const u8,
        c: &ApplyCtx,
    ) -> i32 {
        self.handle().apply_record(cptr(core), kind, body, c)
    }
    unsafe fn remove(&self, core: &FakeCore, owner: u8) {
        self.handle().detach(cptr(core), owner)
    }
    unsafe fn cap(&self, core: &FakeCore, kind: u8) -> Option<(u32, u32)> {
        self.handle().cap(cptr(core), kind)
    }
    unsafe fn attribute(&self, core: &FakeCore, site: u32, id: u32) -> Option<usize> {
        self.handle().attribute(cptr(core), site, id)
    }
    unsafe fn diag(&self, core: &FakeCore, words: &[u32; DIAG_WORDS]) {
        self.handle().diag(cptr(core), words)
    }
    unsafe fn table_check(&self, core: &FakeCore, used: u32, out: &mut TableFault) -> u32 {
        self.handle().table_check(cptr(core), used, out)
    }
    unsafe fn on_loaded(&self, core: &FakeCore, rec: usize) {
        self.handle().on_loaded(cptr(core), rec)
    }
    unsafe fn tick(&self, core: &FakeCore, phase: u32) {
        self.handle().tick(cptr(core), phase)
    }
    unsafe fn module_begin(&self, core: &FakeCore, rec: usize) {
        self.handle().module_begin(cptr(core), rec)
    }
    unsafe fn module_commit(&self, core: &FakeCore, rec: usize, c: &ModuleCommitCtx) -> i32 {
        self.handle().module_commit(cptr(core), rec, c)
    }
    unsafe fn settle(&self, core: &FakeCore, s: &SettleCtx) {
        self.handle().settle(cptr(core), s)
    }
    unsafe fn dsp_check_sections(&self, core: &FakeCore, sec: &DspSec, job: &mut Job) -> i32 {
        self.handle().dsp_check_sections(cptr(core), sec, job)
    }
    unsafe fn dsp_check_rows(&self, core: &FakeCore, sec: &DspSec) -> i32 {
        self.handle().dsp_check_rows(cptr(core), sec)
    }
    unsafe fn dsp_check_types(&self, core: &FakeCore, sec: &DspSec) -> i32 {
        self.handle().dsp_check_types(cptr(core), sec)
    }
    unsafe fn dsp_admit(&self, core: &FakeCore, job: &Job) -> i32 {
        self.handle().dsp_admit(cptr(core), job)
    }
    unsafe fn dsp_claim(&self, core: &FakeCore, c: &mut Claim) {
        self.handle().dsp_claim(cptr(core), c)
    }
    unsafe fn dsp_release(&self, core: &FakeCore, rec: usize) {
        self.handle().dsp_release(cptr(core), rec)
    }
    unsafe fn dsp_clear(&self, core: &FakeCore, rec: usize) -> bool {
        self.handle().dsp_clear(cptr(core), rec)
    }
    unsafe fn dsp_row_step(
        &self,
        core: &FakeCore,
        rec: usize,
        want: &mut u32,
        write: bool,
    ) -> Option<RowWrite> {
        self.handle().dsp_row_step(cptr(core), rec, want, write)
    }
}
