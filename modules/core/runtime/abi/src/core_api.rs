//! Interface C of docs/ARCHITECTURE.md: what a service (`src/services/*` of the runtime crate) calls in the core. A service reaches parameter rows, the
//! registry's and the DSP loader's state, the module table, settings, the store, the log, the job queue and the DSP link only through `CoreApi`, or through the
//! pure types of this crate.
//!
//! `RealCore` (runtime crate) is a zero-sized type that delegates to the core functions, so a call through it costs what the direct call cost. A function
//! that a host test must drive takes a generic `C: CoreApi` (no `dyn`: the runtime has no allocator and no vtables); everything else calls the `REAL_CORE`
//! constant. `FakeCore` (host tests of the runtime crate) records the calls in order.
//!
//! The DSP link belongs to the core (`link.rs` of the runtime crate): a service sets live values with `link_set` and sends owned streams with
//! `link_submit` (types in `crate::link`). It never asks whether the link is busy.
//!
//! Platform facts: the samples service reads the few it needs through `SampleCore::host` (`platform::SampleHost`); `SampleCore` extends `CoreApi` and is not part
//! of interface C of a `.dks` (no import carries a platform handle). The device profile comes with the device id.
use crate::dsp::DspState;
use crate::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
use crate::platform::SampleHost;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// The `done` status of a job that was cancelled (`sdk/include/dk/platform/job.h`).
pub const JOB_E_CANCELLED: i32 = -80;

/// The flags of an info-level log record.
pub const LOG_INFO: u16 = dkrt_sdk::logk::L_INFO << dkrt_sdk::logk::LEVEL_SHIFT;

/// `CoreApi` with the platform facts the samples service reads. Only the in-crate samples service takes this bound; a service file has no such core.
pub trait SampleCore: CoreApi {
    /// What this core's platform is: the real one in the runtime, a small fake in a host test.
    type Host: SampleHost;
    /// The platform facts the samples service may read (`SampleHost`).
    fn host(&self) -> &Self::Host;
}

pub trait CoreApi: crate::mem::FwMem {
    // ---- the registry's rows ---------------------------------------------------------------------------------------------
    /// The selection index of the module being loaded (`u32::MAX` outside a module load): a service that planned something per selected module (`Service::settle`) finds
    /// the module's part by it.
    fn boot_index(&self) -> u32;
    /// The sticky machine type map that the selection record kept before the machines service owned it (`MAP_N` words, 0 empty), read once to migrate it to the
    /// service's settings key. Removed one release after.
    fn legacy_type_map(&self) -> [u32; dkrt_sdk::sel::MAP_N];
    /// What the DSP guard counted for machine type `ty`: faults and whether it disabled the type.
    unsafe fn guard_faults(&self, ty: u32) -> u32;
    unsafe fn guard_disabled(&self, ty: u32) -> bool;

    // ---- the device ------------------------------------------------------------------------------------------------------
    /// The device id of the profile the runtime was built for (`Platform::device_id`), to compare with what an adapter table records.
    fn device_id(&self) -> u32;

    // ---- the module table ------------------------------------------------------------------------------------------------
    /// The NUL-terminated id of module record `owner - 1` (`owner` is 1 or more).
    unsafe fn module_name(&self, owner: u8) -> *const u8;
    /// The record of the module whose context is `ctx`, as record index + 1, when that module is the code running now (a call with another module's context, or
    /// from no module, gives 0). The services use it for the handle a module opens.
    unsafe fn module_of(&self, ctx: *const u8) -> u8;

    // ---- settings --------------------------------------------------------------------------------------------------------
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32;
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32;

    // ---- log and source --------------------------------------------------------------------------------------------------
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]);
    /// The module whose code runs now (record index + 1; 0 outside module code).
    fn current_source(&self) -> u8;

    // ---- jobs ------------------------------------------------------------------------------------------------------------
    unsafe fn job_post(
        &self,
        owner: u8,
        step: Option<JobStepFn>,
        done: Option<JobDoneFn>,
        ctx: *mut u8,
    ) -> i32;
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32;

    // ---- the DSP loader's state and tables -------------------------------------------------------------------------------
    /// The DSP loader's state and row table.
    fn dsp(&self) -> &'static mut DspState;
    /// The words of `dyndsp_info` (voice rows, the area); null without it.
    unsafe fn dsp_info(&self) -> *const u32;
    /// The words of `dynfx_info` (effect rows); null without it.
    unsafe fn fx_info(&self) -> *const u32;
    /// The words of the entry `name_hash` of the image's one table `dkrt_info_table` (`dkneeds::keys::name_hash` of the name that an adapter's `[[info]]` gives): the data
    /// table of an image adapter, such as `fxpages_info`. Null when the image has no such entry or the core does not serve it. The caller checks the words (magic, device).
    /// The core forwards to `Platform::info`; it names no service and no table.
    unsafe fn info(&self, name_hash: u32) -> *const u32;

    // ---- the DSP link ----------------------------------------------------------------------------------------------------
    /// Set the live value of `slot`. The core sends it (the newest value only) and sends it again when a write failed.
    fn link_set(&self, slot: Slot, value: u32);
    /// The value of `slot` that the DSP has taken (`None`: none yet, or the DSP restarted since).
    fn link_confirmed(&self, slot: Slot) -> Option<u32>;
    /// Live writes that failed so far (each is sent again).
    fn link_errors(&self) -> u32;
    /// Drop the live values of the `count` words from `first` of window `win` (their row is gone): nothing more is sent for them.
    fn link_forget(&self, win: u32, first: u32, count: u32);
    /// Queue a request of `owner`. `None` when the queue is full or the request is not valid (an empty or odd-sized transfer, a reserved window).
    unsafe fn link_submit(&self, owner: Owner, stream: &Stream) -> Option<Ticket>;
    /// The result of a request, for its owner only. A terminal result is given once.
    fn link_result(&self, t: Ticket) -> LinkResult;
    /// Cancel the queued requests of `owner` and wait for its transfer in flight. `false` when that did not end in time.
    unsafe fn link_drain(&self, owner: Owner) -> bool;
    /// Send `seq` for `owner` and wait for the result (error paths and unload clears). `true` when every step was taken.
    unsafe fn link_run(&self, owner: Owner, seq: &Sequence) -> bool;
}
