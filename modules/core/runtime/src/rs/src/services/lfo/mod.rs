//! `lfo` service: the shapes an LFO is read through, in an owner-tracked table. R7 proves that interface B is generic with this service
//! (docs/plans/runtime-and-services.md R7): a module emits an `lfo_shape` record (`dk_record_add`), the service checks and keeps it, and the registry,
//! the collector, the loader, dkneeds and patchkit know nothing of the kind. A shape has no effect on the firmware: it is the base that real LFOs
//! (modulation targets, maybe a DSP part) build on.
pub mod records;

#[cfg(test)]
mod tests;
