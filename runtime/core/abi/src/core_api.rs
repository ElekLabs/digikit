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
//! Platform facts: a service reads the few it needs through `CoreApi` (`device_id`, `storage_stat`, `storage_stat_head`, `irq_mask`, `info`). The device profile comes with the device id.
use crate::dsp::DspState;
use crate::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
use crate::storage::Stat;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// The `done` status of a job that was cancelled (`sdk/include/dk/platform/job.h`).
pub const JOB_E_CANCELLED: i32 = -80;

/// The flags of an info-level log record.
pub const LOG_INFO: u16 = dkrt_sdk::logk::L_INFO << dkrt_sdk::logk::LEVEL_SHIFT;

pub trait CoreApi: crate::mem::FwMem {
    // ---- the registry's rows ---------------------------------------------------------------------------------------------
    /// The selection index of the module being loaded (`u32::MAX` outside a module load): a service that planned something per selected module (`Service::settle`) finds
    /// the module's part by it.
    fn boot_index(&self) -> u32;
    /// The 32 bytes of the selection record's reserved area that a service kept its state in before it had a settings key (all 0: none), read once to migrate them. Removed one
    /// release after the last service has migrated.
    fn legacy_state(&self) -> [u8; 32];
    /// Zero those bytes in the record on the card, once the service has its state in its own settings key; `OK` or an error (the service tries again at the next boot).
    unsafe fn legacy_state_clear(&self) -> i32;
    /// The keys of record kind `kind` in the header table at selection index `at` of the active or proposed selection (`out` takes at most 8): the keys a service numbers
    /// or checks a record against. None for an index the plan refused, one without a table, and a kind the table does not list. The core reads the table and names no kind.
    fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize;

    /// The module that a live operation changes (`reload::phase`): its owner (module record index + 1), or 0 when none runs. Today the only operation is a reload (E5); E5c
    /// (enable and disable from the menu) will extend the value to say which operation runs. From the checks of the new file until the job ends. A service
    /// leaves that module's own rows out of "used by another module" in its checks of the new file (voice types, rows), parks what the module held in the teardown instead of
    /// freeing it, and at the end of the job (`Service::live_done`) gives back what the new version did not take over.
    fn live_op(&self) -> u32;

    // ---- the device ------------------------------------------------------------------------------------------------------
    /// The device id of the profile the runtime was built for (`Platform::device_id`), to compare with what an adapter table records.
    fn device_id(&self) -> u32;

    // ---- the module table ------------------------------------------------------------------------------------------------
    /// The NUL-terminated id of module record `owner - 1` (`owner` is 1 or more).
    unsafe fn module_name(&self, owner: u8) -> *const u8;
    /// The record of the module whose context is `ctx`, as record index + 1, when that module is the code running now (a call with another module's context, or
    /// from no module, gives 0). The services use it for the handle a module opens.
    unsafe fn module_of(&self, ctx: *const u8) -> u8;

    // ---- the files and the interrupt mask ---------------------------------------------------------------------------------------
    /// `Storage::stat` of the platform (`ROOT_SAMPLES` or `ROOT_STORE`, root-relative path): the services name no platform.
    unsafe fn storage_stat(&self, root: u32, path: &[u8]) -> Result<Stat, i32>;
    /// `Storage::stat_head`: the `Stat` and the first 64 bytes (the count is 0 for the root and 64 for a file).
    unsafe fn storage_stat_head(
        &self,
        root: u32,
        path: &[u8],
        head: &mut [u8; 64],
    ) -> Result<(Stat, usize), i32>;
    /// Mask interrupts; the old status register to give back to `irq_restore`.
    fn irq_mask(&self) -> u32;
    fn irq_restore(&self, sr: u32);

    // ---- settings --------------------------------------------------------------------------------------------------------
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32;
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32;

    // ---- the settings overlay and the live job ----------------------------------------------------------------------------------
    /// A service gives the runtime the table its settings calls forward to (`SettingsOverlay`; valid while the service's file is loaded: the runtime drops it with the record).
    /// Called from the service's `init`; the owner is the source that runs then. One provider at a time.
    unsafe fn settings_overlay_set(&self, overlay: *const crate::service::SettingsOverlay);
    /// The effective settings of module `id` changed behind the module's back (the overlay's values differ from what the module read): the runtime reloads the module when it
    /// opened its settings (`settings_reload`).
    unsafe fn settings_changed(&self, id: &[u8]);
    /// Queue a live job (`LIVE_LOAD`, `LIVE_UNLOAD` or `LIVE_RELOAD` of the file at `path`, NUL terminated): `dkrt_live`, with its refusals. UI task only.
    unsafe fn live(&self, op: u32, path: *const u8) -> i32;
    /// The live job's state (`dkrt_live_state`). UI task only.
    unsafe fn live_state(&self) -> dkrt_sdk::DkrtLive;
    /// Add the store file at `path` to the saved selection (what the next boot loads), as the DIGIKIT menu saves it for a module it loads live: `OK` also when the path is
    /// there already, `E_SEL_COUNT` when the selection is full, `E_SEL_PATH` for a path that is no selection entry, `E_READ` when the record cannot be read. UI task only.
    unsafe fn selection_add(&self, path: &[u8]) -> i32;

    // ---- the loaded modules and the store's module files ------------------------------------------------------------------------
    /// The NUL-terminated store path (at most 63 bytes) that the content module in record `owner - 1` was loaded from (`owner` as for `module_name`); null for a record that
    /// holds no loaded module, and for an `owner` past the module records.
    unsafe fn module_path(&self, owner: u8) -> *const u8;
    /// The store path of the file called `name` under the folder `dir` (`modules/`, any depth below it): its length and the path NUL padded in `out`, or `E_NOT_FOUND`
    /// (also without a store). The store's module listing, so the first match in path order.
    unsafe fn store_find(&self, dir: &[u8], name: &[u8], out: &mut [u8; 64]) -> i32;

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
    /// The DSP loader's state (the module ranges and the load in flight); the voice and effect rows are the services' own.
    fn dsp(&self) -> &'static mut DspState;
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
