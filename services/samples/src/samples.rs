//! `Samples`: the owner object of the service. It holds all state: the tracks of the clients (`Tracks`, with the table writes), the sample-pool adapter (`Pool`) and the peak
//! windows (`PeakWindows`). The file's cell holds the one instance (`Provider::start_in` makes it in the file's `init`); the C entry points a module calls and the two pool
//! event slots reach it with `DK_SVC_OWNER.with(...)`.
//!
//! It implements `Provider` (start), `DspPart` (the track-table record of a module's DSP section: `check_sections`, `admit`, `claim`, `loaded`, `release`) and `Ticker` (one
//! step of every track in the early phase of the UI pass). It owns no record kind of the `register` section.
use crate::dsp;
use crate::peakwin::PeakWindows;
use crate::pool::Pool;
use crate::tracks::Tracks;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{DspPart, Provider, Status, Ticker};
use dkrt_abi::service::{tick_phase, Claim};

pub struct Samples {
    tracks: Tracks,
    pool: Pool,
    peaks: PeakWindows,
}

impl Samples {
    /// A service over the backing `pool` (`Pool::none()`: no adapter, so every `sample_pool_add` refuses).
    pub fn new(pool: Pool) -> Samples {
        Samples {
            tracks: Tracks::new(),
            pool,
            peaks: PeakWindows::new(),
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

    /// `dk_sample_pool_add`.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string.
    pub unsafe fn pool_add<C: CoreApi>(&mut self, core: &C, handle: u32, path: *const u8) -> i32 {
        self.tracks.pool_add(core, &self.pool, handle, path)
    }

    /// `dk_sample_pool_set`.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string.
    pub unsafe fn pool_set<C: CoreApi>(
        &mut self,
        core: &C,
        handle: u32,
        index: u32,
        path: *const u8,
    ) -> i32 {
        self.tracks.pool_set(core, &self.pool, handle, index, path)
    }

    /// `dk_sample_pool_clear`.
    pub fn pool_clear<C: CoreApi>(&mut self, core: &C, handle: u32, index: u32) -> i32 {
        self.tracks.pool_clear(core, handle, index)
    }

    /// `dk_sample_pool_status`.
    pub fn status(&self, handle: u32, index: u32) -> i32 {
        self.tracks.status(handle, index)
    }

    /// `dk_sample_pool_info`.
    ///
    /// # Safety
    /// `out` is null or writable for 16 bytes.
    pub unsafe fn info(&self, handle: u32, index: u32, out: *mut u8) -> i32 {
        self.tracks.info(handle, index, out)
    }

    /// `dk_sample_pool_path`.
    ///
    /// # Safety
    /// `buf` is null or writable for `cap` bytes.
    pub unsafe fn path<C: CoreApi>(
        &self,
        core: &C,
        handle: u32,
        index: u32,
        buf: *mut u8,
        cap: u32,
    ) -> i32 {
        self.tracks.path(core, handle, index, buf, cap)
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

    /// `dk_sample_pool_peaks_window`.
    ///
    /// # Safety
    /// `out` is null or writable for `capacity` bytes.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn peaks_window<C: CoreApi>(
        &mut self,
        core: &C,
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

    /// The pool event `pool_done`: load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
    pub fn pool_done(&mut self, req: u32, result: i32) {
        self.tracks.pool_done(req, result);
    }

    /// The pool event `pool_lost`: the backing was lost, all handles are invalid.
    pub fn pool_lost<C: CoreApi>(&mut self, core: &C) {
        self.tracks.pool_lost(core, &self.pool);
    }
}

impl<C: CoreApi> Provider<C> for Samples {
    /// The adapter's calls come from its table `sample_pool_info`. Without the adapter the file does not load (`requires = core/sample-pool` of its module.toml, so its users are
    /// refused with a missing need); `Pool::none()` here is a host build, where there is no way to call the adapter.
    fn start(core: &C) -> Result<Samples, Status> {
        // SAFETY: the table of the image holds the addresses of the adapter's functions
        Ok(Samples::new(unsafe { Pool::from_info(core) }))
    }

    /// The state is made where it lives: the tracks are 35 kB, so no copy of the value goes through the stack.
    fn start_in(core: &C, slot: &mut MaybeUninit<Samples>) -> Result<(), Status> {
        let p = slot.as_mut_ptr();
        // SAFETY: each field is written whole through its own pointer; the three `init` calls work on `MaybeUninit` views of the fields
        unsafe {
            Tracks::init(&mut *(addr_of_mut!((*p).tracks) as *mut MaybeUninit<Tracks>));
            PeakWindows::init(&mut *(addr_of_mut!((*p).peaks) as *mut MaybeUninit<PeakWindows>));
            addr_of_mut!((*p).pool).write(Pool::from_info(core));
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

    fn claim(&mut self, _core: &C, c: &mut Claim) {
        // SAFETY: as `admit`
        unsafe { dsp::claim(&mut self.tracks, c) }
    }

    /// Give back the client slots of the module: its tracks are unbound, the pool references released and its peak window stopped.
    fn release(&mut self, core: &C, module: usize) {
        let freed = self.tracks.release(core, &self.pool, module);
        let mut client = 0;
        while client < crate::tracks::CLIENTS {
            if freed >> client & 1 != 0 {
                self.peaks.reset(core, client);
            }
            client += 1;
        }
    }

    /// Every row of the module is in the DSP: queued table writes may go out.
    fn loaded(&mut self, _core: &C, module: usize) {
        self.tracks.ready(module);
    }
}

impl<C: CoreApi> Ticker<C> for Samples {
    fn tick(&mut self, core: &C, phase: u32) {
        if phase == tick_phase::EARLY {
            self.tracks.tick(core, &self.pool);
        }
    }
}
