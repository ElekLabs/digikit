use super::install::Install;
use super::install_step::InstallStep;
use super::stats::Stats;
use super::step::Step;
use super::table::{BindFn, InstallFn, RetireFn, StatsFn, Table, FLAG_STEREO, MAGIC, MAJOR, MINOR};
use crate::{E_CONTRACT, E_FORMAT, E_MISSING};
use core::ffi::c_void;

/// A checked VoiceHost v1 table. All calls run in the UI task and do not block.
#[derive(Clone, Copy)]
pub struct VoiceHost {
    tracks: u16,
    types_max: u16,
    payload_bytes_max: u32,
    state_bytes_max: u32,
    budget: u32,
    flags: u32,
    ctx: *mut c_void,
    install: InstallFn,
    bind: BindFn,
    retire: RetireFn,
    stats: StatsFn,
}

impl VoiceHost {
    /// Checks the table the image info table gave for `voice_host`.
    ///
    /// Errors: `E_MISSING` for a null pointer, `E_FORMAT` for a bad magic, a size that does not cover the
    /// fields, or a null function pointer, `E_CONTRACT` for another major or a minor below the one this crate knows.
    ///
    /// # Safety
    /// A non-null `table` must point to at least three readable words (magic, major and minor, size) and, when its
    /// size field says so, to that many bytes, valid for as long as the result is used.
    pub unsafe fn from_info(table: *const u8) -> Result<Self, i32> {
        if table.is_null() {
            return Err(E_MISSING);
        }
        // Only the three header words are read before the size is checked.
        let head = table.cast::<u32>();
        if head.read() != MAGIC {
            return Err(E_FORMAT);
        }
        let (major, minor) = (
            table.add(4).cast::<u16>().read(),
            table.add(6).cast::<u16>().read(),
        );
        if major != MAJOR || minor < MINOR {
            return Err(E_CONTRACT);
        }
        if (head.add(2).read() as usize) < core::mem::size_of::<Table>() {
            return Err(E_FORMAT);
        }
        let t = table.cast::<Table>().read();
        match (t.install, t.bind, t.retire, t.stats) {
            (Some(install), Some(bind), Some(retire), Some(stats)) => Ok(Self {
                tracks: t.tracks,
                types_max: t.types_max,
                payload_bytes_max: t.payload_bytes_max,
                state_bytes_max: t.state_bytes_max,
                budget: t.budget,
                flags: t.flags,
                ctx: t.ctx,
                install,
                bind,
                retire,
                stats,
            }),
            _ => Err(E_FORMAT),
        }
    }

    pub fn tracks(&self) -> u16 {
        self.tracks
    }
    pub fn types_max(&self) -> u16 {
        self.types_max
    }
    pub fn payload_bytes_max(&self) -> u32 {
        self.payload_bytes_max
    }
    /// Per voice.
    pub fn state_bytes_max(&self) -> u32 {
        self.state_bytes_max
    }
    /// Cost units per block the host admits.
    pub fn budget(&self) -> u32 {
        self.budget
    }
    pub fn stereo(&self) -> bool {
        self.flags & FLAG_STEREO != 0
    }

    /// Installs a voice type. `requested` is the type the service asks for: non-zero is used exactly or the call is refused (it never
    /// answers another type), zero lets the host choose. `Done(type)` finished; `Loading(type)` is not finished and `type` is the type
    /// the request occupies (chosen now when `requested` was 0). `Loading(0)` means the host is busy with another install and did not
    /// accept this call: call again later with the same request, payload included.
    ///
    /// Continuation rule: the call after `Loading` passes that type as `requested` and [`Install::continuation`] (a null payload).
    /// A call with a null payload and the type of the install in progress is the continuation; any other call, also one identical to
    /// the first, answers `Loading(0)` while an install is in progress: the host has one staging buffer.
    /// The in-flight install ends with `Done`, an error, or `retire` of its type, which frees the buffer for the next install.
    ///
    /// A request with no payload (an empty slice) installs the silent voice, or parks an installed type in place; a later install with
    /// a payload takes it over. The host copies the payload during the first call. Refused with `E_NOMEM` when the voices do not fit
    /// the host. Once a call returns, the host holds no pointer into the request, the payload or any service or module memory. It
    /// keeps no callbacks into a service, so a service reload can retire its types and install them again.
    pub fn install(&self, request: &Install<'_>, requested: u32) -> Result<InstallStep, i32> {
        let mut ty = requested;
        // SAFETY: the pointers were checked non-null at `from_info`; `request` and `ty` are live for the call.
        let status = unsafe { (self.install)(self.ctx, request, &mut ty) };
        match Step::from_status(status, || ty)? {
            Step::Done(ty) => Ok(InstallStep::Done(ty)),
            Step::Loading => Ok(InstallStep::Loading(ty)),
        }
    }

    /// Selects `ty` for `track` (type 0: the stock machine).
    pub fn bind(&self, track: u32, ty: u32) -> Result<Step<()>, i32> {
        // SAFETY: as in `install`.
        let status = unsafe { (self.bind)(self.ctx, track, ty) };
        Step::from_status(status, || ())
    }

    /// Releases a type, also a silent or parked one. `Loading`: call again, like `install`. While a type retires, every track bound to it plays the stock fallback
    /// (silent for digikit). The host releases the type's placed code and voice state only when `retire` returns `Done`. Keep calling
    /// until `Done` or an error.
    pub fn retire(&self, ty: u32) -> Result<Step<()>, i32> {
        // SAFETY: as in `install`.
        let status = unsafe { (self.retire)(self.ctx, ty) };
        Step::from_status(status, || ())
    }

    /// The counters of a type.
    pub fn stats(&self, ty: u32) -> Result<Step<Stats>, i32> {
        let mut out = Stats::default();
        // SAFETY: as in `install`.
        let status = unsafe { (self.stats)(self.ctx, ty, &mut out) };
        Step::from_status(status, || out)
    }
}
