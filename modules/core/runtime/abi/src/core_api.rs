//! Interface C of docs/ARCHITECTURE.md: what a service (`src/services/*` of the runtime crate) calls in the core. A service reaches parameter rows, the
//! registry's and the DSP loader's state, the module table, settings, the store, the log, the job queue and the DSP link only through `CoreApi`, or through the
//! pure types of this crate.
//!
//! `RealCore` (runtime crate) is a zero-sized type that delegates to the core functions, so a call through it costs what the direct call cost. A function
//! that a host test must drive takes a generic `C: CoreApi` (no `dyn`: the runtime has no allocator and no vtables); everything else calls the `REAL_CORE`
//! constant. `FakeCore` (host tests of the runtime crate) records the calls in order.
//!
//! The DSP link is one transfer at a time. Three writers hold it across polls: the effect page values (`LinkWriter::FxPage`), the send amounts
//! (`LinkWriter::FxSend`) and the sample track writer (`LinkWriter::Sample`). `link_busy` says that an effect writer has a transfer in flight (the
//! sample writer checks it before it starts one); `link_claimed` asks for one writer (the effect writers check the sample writer). Interim until the R6
//! transfer queue.
//!
//! Platform facts (the device profile, the interrupt mask) are not here: they belong to `platform::Platform`. A service names them through `crate::core_api::PLATFORM`.
use crate::dsp::DspState;
use crate::registry::RegState;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// The `done` status of a job that was cancelled (`sdk/include/dk/platform/job.h`).
pub const JOB_E_CANCELLED: i32 = -80;

/// The flags of an info-level log record.
pub const LOG_INFO: u16 = dkrt_sdk::logk::L_INFO << dkrt_sdk::logk::LEVEL_SHIFT;

/// Who starts a transfer on the DSP link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkWriter {
    /// The effect pages' knob and MIX words.
    FxPage,
    /// The send amount words.
    FxSend,
    /// The sample tracks' table writes.
    Sample,
}

impl LinkWriter {
    /// The claim bit a writer holds while its transfer is in flight.
    pub const fn claim(self) -> u8 {
        match self {
            LinkWriter::FxPage => 1,
            LinkWriter::FxSend => 2,
            LinkWriter::Sample => 4,
        }
    }
}

/// The claim bits of the effect writers.
pub const FX_CLAIMS: u8 = 3;

pub trait CoreApi {
    // ---- the registry's rows ---------------------------------------------------------------------------------------------
    /// The words of `dynreg_info` (`registry::w`); null without a registry.
    unsafe fn dynreg(&self) -> *const u32;
    /// The ownership and string state of the reserved machine, parameter and page rows.
    fn reg_state(&self) -> &'static mut RegState;
    /// The code of the first run-time parameter row; `u32::MAX` without a registry.
    fn param_first(&self) -> u32;
    /// The owner (module record + 1, 0 free) of each run-time parameter row.
    fn param_owners(&self) -> &[u8];
    /// Write one parameter table row from its lowering spec (`inf` is `dynreg()`).
    unsafe fn write_param_row(
        &self,
        inf: *const u32,
        code: u32,
        template: u32,
        slot: u32,
        min: u32,
        max: u32,
        def: u32,
        name: u32,
        short: u32,
    );
    unsafe fn write_fx_param_row(
        &self,
        code: u32,
        template: u32,
        mirror: u32,
        min: u32,
        max: u32,
        def: u32,
        name: u32,
        short: u32,
    );
    unsafe fn write_send_param_row(
        &self,
        code: u32,
        class: u32,
        owner_of: u32,
        mirror: u32,
        name: u32,
        short: u32,
    );
    unsafe fn reset_param_row(&self, code: u32);
    /// A firmware string representation of `src` at `rep`; the address of its characters.
    unsafe fn make_rep(&self, rep: *mut u8, src: *const u8, upper: bool) -> u32;
    /// The machine type number for machine key `key` of the module being loaded (sticky assignment); 0 when there is none.
    unsafe fn type_resolve(&self, key: u32) -> u32;
    /// Entries of the sticky type map in use (for CAPACITY).
    fn type_map_used(&self) -> u32;
    /// What the DSP guard counted for machine type `ty`: faults and whether it disabled the type.
    unsafe fn guard_faults(&self, ty: u32) -> u32;
    unsafe fn guard_disabled(&self, ty: u32) -> bool;

    // ---- the module table ------------------------------------------------------------------------------------------------
    /// The NUL-terminated id of module record `owner - 1` (`owner` is 1 or more).
    unsafe fn module_name(&self, owner: u8) -> *const u8;

    // ---- settings --------------------------------------------------------------------------------------------------------
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32;
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32;

    // ---- the store -------------------------------------------------------------------------------------------------------
    /// Look the digikit store file `path` (NUL-terminated) up: its size and SHA-256, and when `head` is given its first 64 bytes. The negative error otherwise.
    unsafe fn store_probe(
        &self,
        path: *const u8,
        head: Option<&mut [u8; 64]>,
    ) -> Result<(u32, [u8; 32]), i32>;

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

    // ---- the DSP link ----------------------------------------------------------------------------------------------------
    /// Start the write of word `word` of window `win` for `who`. `true` when it was accepted.
    unsafe fn link_word(&self, who: LinkWriter, win: u32, word: u32, value: u32) -> bool;
    /// Write one word of window `win` and wait for it (a load is in progress or the module is being released). `false` when it failed.
    unsafe fn link_word_blocking(&self, win: u32, word: u32, value: u32) -> bool;
    /// Start a serialized window-zero transfer of `len` bytes at `src` to `dst`: 0 accepted, -2 refused for now, another negative error.
    unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32;
    /// The result of the transfer `who` started: `Some(true)` done, `Some(false)` failed, `None` still running.
    unsafe fn link_poll(&self, who: LinkWriter) -> Option<bool>;
    /// No load and no transfer is in progress on the link.
    unsafe fn link_idle(&self) -> bool;
    /// An effect page value or a send amount transfer is in flight.
    fn link_busy(&self) -> bool;
    /// `who` has a transfer in flight.
    fn link_claimed(&self, who: LinkWriter) -> bool;
}
