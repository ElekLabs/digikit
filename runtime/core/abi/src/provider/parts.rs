//! The optional parts of the service contract. Each one is a separate tagged table in the packed row (`row::PART_*`); a service adopts only the parts it needs.
use super::{Consumer, ConsumerId, Status, WordSink};
use crate::core_api::CoreApi;
use crate::service::Problem;

/// Check the records of one kind. Callable without a device (`patchkit records --check` runs the same rule before load).
pub trait Records {
    /// `bodies` are the bodies of the records of `kind` of one consumer, in file order; each fault goes to `report`.
    fn validate(kind: u16, bodies: &[&[u8]], report: &mut dyn FnMut(Problem));
}

/// DSP admission of a consumer's rows.
pub trait DspPart<C: CoreApi> {
    fn admit(&mut self, core: &C, consumer: &Consumer<'_>) -> Result<(), Status>;
    fn release(&mut self, core: &C, who: ConsumerId);
}

/// Work in the UI-loop tick; `phase` is one of `service::tick_phase`.
pub trait Ticker<C: CoreApi> {
    fn tick(&mut self, core: &C, phase: u32);
}

/// A synchronous request from a resident app, in the UI task. The service owns operation numbers and layouts.
pub trait Query {
    /// The length of the reply written to `reply`.
    fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> Result<usize, Status>;
}

/// The words a consumer contributes to diagnostics.
pub trait Describe {
    fn describe(&self, who: ConsumerId, out: &mut WordSink);
}
