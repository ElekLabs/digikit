//! `samples` service: per-track stereo samples, the sample-pool calls and the peak windows (docs/plans/runtime-slimming.md, RS-S). A crate of its own, linked into the
//! service file `services/samples.dks` (`dks/`), never into the runtime. It reaches the core only through `dkrt-abi`: `CoreApi` (interface C: the link, the DSP loader
//! state, the job queue, the log, `module_of`, `storage_stat`, `storage_stat_head`, `irq_mask`, `device_id` and `info`). Every function that needs the core takes it by
//! reference (`core: &C`, `C: CoreApi`): the service file passes its `ImportCore` and a host test a `FakeCore`.
//!
//! Owner objects, each with private state:
//! - [`samples::Samples`]: the owner of the service, one instance in the file's cell. It holds the three below and implements `Provider` (start), `DspPart` (the track-table
//!   record of a module's DSP section) and `Ticker` (one step of every track).
//! - `tracks::Tracks`: the clients' tracks. The state machine of a track is `dkrt_sdk::sample_tracks`; `tracks` binds it to the backing adapter and to the table writes. The
//!   tables `DKRT_SMP_RECORDS` and `DKRT_SMP_CONTROL` stay named statics, because device tests read them by symbol; `Tracks::reserve` is their only writer.
//! - `writes::Writes`: the table writes over the DSP link (close, publish), one in flight at a time.
//! - [`pool::Pool`]: the backing adapter (the calls of `dkrt_pool.h` from the adapter's table `sample_pool_info`).
//! - `peakwin::PeakWindows`: the peak windows, run on the job queue.
//!
//! Modules: `files` (the sample root of the device and the identity of a file), [`dsp`] (check and claim of the track-table record of a DSP section). The C entry points
//! (`dk_sample_pool_*` and the two pool event slots) are in `dks/src/glue.rs`.
#![no_std]
#[cfg(test)]
extern crate std;
pub mod dsp;
pub mod events;
mod files;
mod peakwin;
pub mod pool;
pub mod samples;
mod tracks;
mod writes;
