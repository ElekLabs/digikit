//! Providers for the host tests of the core's walks and loader seams: an owner type that implements the parts it needs becomes a resolved row (`ProviderHandle`, or the
//! `RowSource` the registry takes), as a service file's `service!` makes one. The owner is a zero-size type that keeps its state in statics (the part functions reach an
//! instance that is a leaked block of bytes), so a test that watches calls names its own static log.
use crate::package::core_api::{RealCore, REAL_CORE};
use crate::package::provider_rows::RowSource;
use dkrt_abi::provider::{
    name_hash, Describe, DescribeV1, DspPart, DspV1, LifecycleV1, PartRef, Payloads, PayloadsV1, Provider, ProviderHandle, ProviderRow, Records, RecordsV1, TickV1, Ticker, DESCRIBE_NEWEST,
    DSP_NEWEST, LIFECYCLE_NEWEST, PART_DESCRIBE, PART_DSP, PART_LIFECYCLE, PART_PAYLOADS, PART_RECORDS, PART_TICK, PAYLOADS_NEWEST, RECORDS_NEWEST, TICK_NEWEST,
};
use std::boxed::Box;
use std::vec::Vec;

/// A row under construction: the lifecycle part of `P` and the optional parts added by name.
pub(crate) struct Fixture {
    name: &'static str,
    parts: Vec<PartRef>,
}

impl Fixture {
    /// A service called `name` over owner type `P` with the lifecycle part alone.
    pub(crate) fn of<P: Provider<RealCore>>(name: &'static str) -> Fixture {
        let life: &'static LifecycleV1 = Box::leak(Box::new(LifecycleV1::of::<P, RealCore>()));
        Fixture { name, parts: std::vec![PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, life as *const _ as *const ())] }
    }

    /// The records part, owning `kinds` (kind, whole record length).
    pub(crate) fn records<P: Provider<RealCore> + Records<RealCore>>(mut self, kinds: &'static [(u32, u32)]) -> Fixture {
        let t: &'static RecordsV1 = Box::leak(Box::new(RecordsV1::of::<P, RealCore>(kinds)));
        self.parts.push(PartRef::new(PART_RECORDS, RECORDS_NEWEST, t as *const _ as *const ()));
        self
    }

    pub(crate) fn dsp<P: Provider<RealCore> + DspPart<RealCore>>(mut self) -> Fixture {
        let t: &'static DspV1 = Box::leak(Box::new(DspV1::of::<P, RealCore>()));
        self.parts.push(PartRef::new(PART_DSP, DSP_NEWEST, t as *const _ as *const ()));
        self
    }

    pub(crate) fn payloads<P: Provider<RealCore> + Payloads<RealCore>>(mut self) -> Fixture {
        let t: &'static PayloadsV1 = Box::leak(Box::new(PayloadsV1::of::<P, RealCore>()));
        self.parts.push(PartRef::new(PART_PAYLOADS, PAYLOADS_NEWEST, t as *const _ as *const ()));
        self
    }

    pub(crate) fn tick<P: Provider<RealCore> + Ticker<RealCore>>(mut self) -> Fixture {
        let t: &'static TickV1 = Box::leak(Box::new(TickV1::of::<P, RealCore>()));
        self.parts.push(PartRef::new(PART_TICK, TICK_NEWEST, t as *const _ as *const ()));
        self
    }

    pub(crate) fn describe<P: Provider<RealCore> + Describe<RealCore>>(mut self) -> Fixture {
        let t: &'static DescribeV1 = Box::leak(Box::new(DescribeV1::of::<P, RealCore>()));
        self.parts.push(PartRef::new(PART_DESCRIBE, DESCRIBE_NEWEST, t as *const _ as *const ()));
        self
    }

    /// The row a file declares: leaked, with a leaked instance block (the owner types here are zero-size).
    pub(crate) fn row(self) -> &'static ProviderRow {
        let parts: &'static [PartRef] = Box::leak(self.parts.into_boxed_slice());
        Box::leak(Box::new(ProviderRow::new(name_hash(self.name), parts)))
    }

    /// The row as the registry takes it: the extent is the whole address space (a host test's statics are in no package image).
    pub(crate) fn source(self) -> RowSource<'static> {
        RowSource { row: self.row(), instance: instance(), extent: 0..usize::MAX }
    }

    /// The row resolved for a walk of its own (`ServiceTable::new(&[handle], ..)`).
    pub(crate) fn handle(self) -> ProviderHandle {
        let source = self.source();
        // SAFETY: the row, its tables and the instance are leaked; the extent is the whole address space.
        unsafe { source.resolve() }.expect("a fixture row resolves")
    }
}

/// A leaked instance block for an owner type of size zero.
pub(crate) fn instance() -> *mut u8 {
    Box::leak(Box::new([0u8; 8])).as_mut_ptr()
}

/// A leaked table of the handles of `rows`, in the order given, as a walk takes it.
pub(crate) fn table(rows: Vec<ProviderHandle>) -> dkrt_abi::service::ServiceTable {
    let rows: &'static [ProviderHandle] = Box::leak(rows.into_boxed_slice());
    dkrt_abi::service::ServiceTable::new(rows, &REAL_CORE)
}

/// An owner that does nothing: a lifecycle part alone. A service called `name` that owns `kinds` also has the records part (all defaults).
pub(crate) struct Nothing;

impl<C: dkrt_abi::core_api::CoreApi> Provider<C> for Nothing {
    fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<Nothing>) -> Result<(), i32> {
        slot.write(Nothing);
        Ok(())
    }
}

impl<C: dkrt_abi::core_api::CoreApi> Records<C> for Nothing {}

impl Nothing {
    /// A service `name` with no record kind.
    pub(crate) fn named(name: &'static str) -> Fixture {
        Fixture::of::<Nothing>(name)
    }

    /// A service `name` that owns `kinds` and does nothing with them.
    pub(crate) fn owning(name: &'static str, kinds: &'static [(u32, u32)]) -> Fixture {
        Fixture::of::<Nothing>(name).records::<Nothing>(kinds)
    }
}
