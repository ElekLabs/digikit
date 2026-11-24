//! `Samples`: the owner object of the service. It holds all state: the tracks of the clients (`Tracks`, with the table writes), the sample-pool adapter (`Pool`) and the peak
//! windows (`PeakWindows`). The file's cell holds the one instance (`Provider::start_in` makes it in the file's `init`); the C entry points a module calls and the two pool
//! event slots reach it with `DK_SVC_OWNER.with(...)`.
//!
//! It implements `Provider` (start), `DspPart` (the track-table record of a module's DSP section: `check_sections`, `admit`, `claim`, `loaded`, `release`) and `Ticker` (one
//! step of every track in the early phase of the UI pass). It owns no record kind of the `register` section.
use crate::dsp;
use crate::peakwin::{PeakJob, PeakJobs, PeakWindows};
use crate::pool::Pool;
use crate::table::{PoolCell, PoolTable, POOL_TABLE};
use crate::tracks::Tracks;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{DspPart, Provider, Status, Ticker};
use dkrt_abi::service::{tick_phase, Claim};
use dkrt_sdk::api::JobDoneFn;

pub struct Samples {
    tracks: Tracks,
    pool: Pool,
    peaks: PeakWindows,
    /// The pool table (`table.rs`): a cell outside this object, shared with the exports that answer at once. The owner meets it in short sections.
    table: &'static PoolCell,
}

impl Samples {
    /// A service over the backing `pool` (`Pool::none()`: no adapter, so every `sample_pool_add` refuses) and the pool table `table`. `Provider::start` empties the table and tells
    /// it whether an adapter is installed; this constructor leaves the table as it is.
    pub fn new(pool: Pool, table: &'static PoolCell) -> Samples {
        Samples {
            tracks: Tracks::new(),
            pool,
            peaks: PeakWindows::new(),
            table,
        }
    }

    /// Does module record `rec` hold a track table?
    pub fn has_table(&self, rec: usize) -> bool {
        self.tracks.has(rec)
    }

    /// Is the table of module record `rec` live (its DSP image is in, so its writes may go out)?
    pub fn table_is_live(&self, rec: usize) -> bool {
        self.tracks.is_live(rec)
    }

    /// The handle of the calling module's table with local id `local_id` (`E_NOT_FOUND` without one).
    pub fn tracks<C: CoreApi>(&self, core: &C, local_id: u32) -> i32 {
        self.tracks.tracks(core, local_id)
    }

    /// `dk_sample_pool_open`: the handle of the calling module's table with the lowest local id.
    pub fn open_first<C: CoreApi>(&self, core: &C) -> i32 {
        self.tracks.open_first(core)
    }

    /// `dk_sample_pool_peaks`.
    ///
    /// # Safety
    /// `out` is null or writable for `capacity` bytes.
    pub unsafe fn peaks(
        &self,
        handle: u32,
        index: u32,
        token: u32,
        out: *mut u8,
        capacity: u32,
    ) -> i32 {
        self.tracks
            .peaks(&self.pool, handle, index, token, out, capacity)
    }

    /// `dk_sample_pool_peaks_window`. `jobs` is the cells of the peak-window jobs and `done` the job-end entry that posts the end to the owner (both the file's).
    ///
    /// # Safety
    /// `out` is null or writable for `capacity` bytes.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn peaks_window<C: CoreApi>(
        &mut self,
        core: &C,
        jobs: &PeakJobs,
        done: JobDoneFn,
        handle: u32,
        index: u32,
        token: u32,
        first: u32,
        frames: u32,
        columns: u32,
        out: *mut u8,
        capacity: u32,
    ) -> i32 {
        self.peaks.window(
            core,
            jobs,
            done,
            &self.tracks,
            &self.pool,
            handle,
            index,
            token,
            first,
            frames,
            columns,
            out,
            capacity,
        )
    }

    /// The pool event `pool_done` (applied by `Reentry::apply`, after the call it arrived in): load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
    pub(crate) fn pool_done(&mut self, req: u32, result: i32) {
        self.tracks.pool_done(req, result);
    }

    /// The pool event `PeakDone` (applied by `Reentry::apply`): a peak-window job ended; its window takes the result or starts the newer request.
    pub(crate) fn peak_done<C: CoreApi>(&mut self, core: &C, job: &PeakJob) {
        self.peaks
            .finish(core, self.tracks.core(), job, self.pool.window_step());
    }

    /// The pool event `pool_lost` (applied by `Reentry::apply`): the backing was lost, all handles are invalid.
    pub(crate) fn pool_lost<C: CoreApi>(&mut self, core: &C) {
        // the calls of a module that came before the loss are applied first, as they were when they changed the machine at once
        self.tracks.intake(core, self.table);
        self.tracks.pool_lost(core, &self.pool);
        self.tracks.mirror(core, self.table);
    }
}

impl<C: CoreApi> Provider<C> for Samples {
    /// The adapter's calls come from its table `sample_pool_info`. Without the adapter the file does not load (`requires = core/sample-pool` of its module.toml, so its users are
    /// refused with a missing need); `Pool::none()` here is a host build, where there is no way to call the adapter.
    fn start(core: &C) -> Result<Samples, Status> {
        // SAFETY: the table of the image holds the addresses of the adapter's functions
        let pool = unsafe { Pool::from_info(core) };
        PoolTable::reset(&POOL_TABLE, core, pool.present());
        Ok(Samples::new(pool, &POOL_TABLE))
    }

    /// The state is made where it lives: the tracks are 35 kB, so no copy of the value goes through the stack.
    fn start_in(core: &C, slot: &mut MaybeUninit<Samples>) -> Result<(), Status> {
        let p = slot.as_mut_ptr();
        // SAFETY: each field is written whole through its own pointer; the three `init` calls work on `MaybeUninit` views of the fields
        unsafe {
            Tracks::init(&mut *(addr_of_mut!((*p).tracks) as *mut MaybeUninit<Tracks>));
            PeakWindows::init(&mut *(addr_of_mut!((*p).peaks) as *mut MaybeUninit<PeakWindows>));
            let pool = Pool::from_info(core);
            addr_of_mut!((*p).pool).write(pool);
            addr_of_mut!((*p).table).write(&POOL_TABLE);
            PoolTable::reset(&POOL_TABLE, core, pool.present());
        }
        Ok(())
    }
}

impl<C: CoreApi> DspPart<C> for Samples {
    fn check_sections(&mut self, _core: &C, sec: &DspSec, _job: &mut Job) -> Result<(), Status> {
        // SAFETY: the section was bounds-checked by the loader before its parts run
        unsafe { dsp::check_sections(sec) }
    }

    fn admit(&mut self, _core: &C, job: &Job) -> Result<(), Status> {
        // SAFETY: `job.bank` is null or points at the record the section check accepted
        unsafe { dsp::admit(&self.tracks, job) }
    }

    fn claim(&mut self, core: &C, c: &mut Claim) {
        // SAFETY: as `admit`
        unsafe { dsp::claim(core, self.table, &mut self.tracks, c) }
    }

    /// Give back the client slots of the module: its tracks are unbound, the pool references released and its peak window stopped.
    fn release(&mut self, core: &C, module: usize) {
        // the window of each client is stopped in the walk of its teardown, before its slot is free (as the old `remove` did)
        let peaks = &mut self.peaks;
        self.tracks
            .release(core, &self.pool, module, |client| peaks.reset(core, client));
        self.tracks.mirror(core, self.table);
    }

    /// Every row of the module is in the DSP: queued table writes may go out.
    fn loaded(&mut self, _core: &C, module: usize) {
        self.tracks.ready(module);
    }
}

impl Samples {
    /// The adapter's `dkpool_tick` of one pass, over `pool`: in the early phase only. The adapter delivers completions and finds a stock reset here, before the tracks step. It fires
    /// `pool_done` and `pool_lost`, which reach the owner as events (`events.rs`).
    pub fn pre_tick_with(pool: &Pool, phase: u32) {
        if phase == tick_phase::EARLY {
            // SAFETY: the adapter's call
            unsafe { pool.tick() };
        }
    }
}

impl<C: CoreApi> Ticker<C> for Samples {
    /// The adapter's tick runs before the owner is entered (the table's tick function calls this first, outside the guard), so its events run at once, with the owner idle, and apply
    /// before the writes pump and the track step of this pass, as before the owner had a guard. The adapter comes from the image's table (`sample_pool_info`), as at `start`.
    fn pre_tick(core: &C, phase: u32) {
        // SAFETY: the table of the image holds the addresses of the adapter's functions
        Self::pre_tick_with(&unsafe { Pool::from_info(core) }, phase)
    }

    fn tick(&mut self, core: &C, phase: u32) {
        if phase == tick_phase::EARLY && self.pool.present() {
            // the requests of the calls of a module first, then one step of every track, then what a call reads is the new state
            self.tracks.intake(core, self.table);
            self.tracks.tick(core, &self.pool);
            self.tracks.mirror(core, self.table);
        }
    }
}

#[cfg(test)]
mod tests;
