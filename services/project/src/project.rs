//! `Project`: the owner object of the service. It holds the engine ([`ProjectService`]) and the host state of the service file ([`Host`]: the SDK handles, the pass count, the
//! listing in progress), and implements `Provider`. The file's cell (`DK_SVC_OWNER`, made by `service!`) holds the one instance: `start` makes it in the file's `init`, `stop`
//! ends it in `fini`. The hooks and exports of the file reach it through the cell; nothing else holds the state.
//!
//! The crate stays neutral: the SDK calls are behind [`Host`], which the file implements (`dks/src/glue/host.rs`) and a host test implements over fakes.
use crate::engine::{Env, Pending, ProjectService, StoreJob, CARRY_MAX, CARRY_VERSION};
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{Provider, State, Status};
use dkrt_abi::service::SettingsOverlay;
use dkrt_sdk::E_BUSY;

/// What the owner needs from the service file besides the engine: the engine's environment (`Env`, in the UI task), the handles it opens at the start and the table of the
/// settings overlay. The write job's file client is not here: the I/O task reads it from the file's own statics.
///
/// # Safety
/// All-zero bytes are the initial state of the implementing type: the owner is made in place from zero bytes (`Provider::start_in`; no copy of the state through the stack).
/// `open` runs on that zeroed value.
pub unsafe trait Host: Env {
    /// The first step of the file's `init`: open the handles (the listing client, core/ui when the image has it). An error is the status of `init`; a failed `open` leaves
    /// nothing that needs a drop.
    fn open(&mut self) -> Result<(), Status>;

    /// A UI pass begins (the clock when the image has no block counter).
    fn begin_pass(&mut self);

    /// The hooks' record of the file (a static: the hooks write it from other tasks). The state a reload carries includes it.
    fn hook_record(&self) -> &'static Pending;

    /// The write job's cell of the file (a static: the I/O task works on it). The service may not be taken down while it holds a job.
    fn job(&self) -> &'static StoreJob;

    /// The table that the runtime's settings overlay forwards to (`dkc_settings_overlay_set`).
    fn overlay() -> *const SettingsOverlay;
}

/// The owner object: the engine and the host state. All-zero bytes are its initial state (the cell is `.bss`).
pub struct Project<H: Host> {
    engine: ProjectService,
    host: H,
}

impl<H: Host> Project<H> {
    /// The engine (the current slot, the live image, for a test or a query).
    pub fn engine(&self) -> &ProjectService {
        &self.engine
    }

    /// The host state.
    pub fn host(&mut self) -> &mut H {
        &mut self.host
    }

    /// The tick of the file (the UI task, once per pass): what the hooks recorded, the work in progress, the next piece. `job` is the write job's cell, a static of the file: the I/O
    /// task works on it while this call holds the owner, so it is not a field of it (`StoreJob`).
    pub fn tick(&mut self, pending: &Pending, job: &StoreJob) {
        self.host.begin_pass();
        self.engine.tick(&mut self.host, pending, job);
    }

    /// `dk_settings_get` on the project scope.
    pub fn scope_get(&self, id: &[u8], key: &[u8], out: &mut [u8]) -> i32 {
        self.engine.scope_get(id, key, out)
    }

    /// `dk_settings_put` on the project scope.
    pub fn scope_put(&mut self, id: &[u8], key: &[u8], value: &[u8]) -> i32 {
        self.engine.scope_put(id, key, value)
    }

    /// `dk_settings_remove` on the project scope.
    pub fn scope_remove(&mut self, id: &[u8], key: &[u8]) -> i32 {
        self.engine.scope_remove(id, key)
    }

    /// `dk_project_current`: the slot and the name of the current project, `E_NOT_FOUND` without one.
    pub fn current(&self) -> Result<(u32, [u8; 16]), i32> {
        self.engine.current()
    }

    /// `dk_project_status`.
    pub fn status(&self) -> u32 {
        self.engine.status()
    }
}

impl<C: CoreApi, H: Host> Provider<C> for Project<H> {
    /// The state is made where it lives: all-zero bytes are the initial state (`Host`'s contract, and `ProjectService::new` is zero: `tests/state.rs` checks it), so this is one fill
    /// and no copy of the engine's buffers through the stack. Then the handles open and the settings overlay is set, as `setup` of the file did.
    fn start_in(core: &C, slot: &mut MaybeUninit<Project<H>>) -> Result<(), Status> {
        // SAFETY: every field is valid as zero bytes (see above); nothing in it needs a drop before `open` ran.
        let project = unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            &mut *slot.as_mut_ptr()
        };
        project.host.open()?;
        // SAFETY: the table is a static of the file; `stop` takes it away.
        unsafe { core.settings_overlay_set(H::overlay()) };
        Ok(())
    }

    /// Not while a stage runs, a store job is posted or the hooks' work waits in the queue: the runtime asks again until the tick has finished it.
    fn can_stop(&self, _core: &C) -> Result<(), Status> {
        if self.engine.can_carry(self.host.job()) {
            Ok(())
        } else {
            Err(E_BUSY)
        }
    }

    /// The file's `fini`: the runtime holds no table of this file after it.
    fn stop(&mut self, core: &C) {
        // SAFETY: null takes the table away.
        unsafe { core.settings_overlay_set(core::ptr::null()) };
    }
}

/// The state of a reload of the file: the engine's (`ProjectService::export_state`) and the hooks' record, which is outside the engine and final when the runtime exports (the old
/// copy's hooks are restored by then).
impl<C: CoreApi, H: Host> State<C> for Project<H> {
    const VERSION: u16 = CARRY_VERSION;
    const ACCEPTS_FROM: u16 = CARRY_VERSION;

    fn export_max(&self, _: &C) -> usize {
        CARRY_MAX
    }

    fn export(&mut self, _: &C, out: &mut [u8]) -> usize {
        self.engine.export_state(self.host.hook_record(), out)
    }

    /// Takes the carried bytes; `None` (or bytes of another layout that the runtime let through) starts named and unbound: module writes go to the settings record only.
    fn import(&mut self, _: &C, from: Option<&[u8]>) -> Result<(), Status> {
        self.engine
            .import_state(&self.host, self.host.hook_record(), from);
        Ok(())
    }
}
