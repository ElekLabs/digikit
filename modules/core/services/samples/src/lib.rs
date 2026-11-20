//! `samples` service: per-track stereo samples, the sample-pool calls and the peak windows (docs/plans/runtime-slimming.md, RS-S). A crate of its own, linked into the
//! service file `services/samples.dks` (`dks/`), never into the runtime. It reaches the core only through `dkrt-abi`: `CoreApi` (interface C: the link, the DSP loader
//! state, the job queue, the log, `module_of`, `storage_stat`, `storage_stat_head`, `irq_mask`, `device_id` and `info`). Every function that needs the core takes it by
//! reference (`core: &C`, `C: CoreApi`): the service file passes its `ImportCore` and a host test a `FakeCore`. The state machine of a track is `dkrt_sdk::sample_tracks`;
//! [`tracks`] binds it to the backing adapter ([`pool`]: the calls of `dkrt_pool.h` from the adapter's table `sample_pool_info`) and the table writes over the DSP link,
//! [`peakwin`] runs the peak windows on the job queue, [`dsp`] checks and claims the track-table record of a DSP section. The C entry points (`dk_sample_pool_*` and
//! the two pool event slots) are in `dks/src/glue.rs`.
//! Layout: `records` (the `Service` handlers), `pool`, `slots` (none yet), `dsp/`, `tests/`.
#![no_std]
#[cfg(test)]
extern crate std;
pub mod dsp;
pub mod peakwin;
pub mod pool;
pub mod records;
pub mod slots;
pub mod tracks;

/// The unit tests of this crate that drive its statics (the client table, the write queue, the peak windows) take this lock.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
