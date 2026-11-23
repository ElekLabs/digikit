//! `Project`: the owner object of the service. It holds the engine ([`ProjectService`]) and the host state of the service file ([`Host`]: the SDK handles, the pass count, the
//! listing in progress), and implements `Provider`. The file's cell (`DK_SVC_OWNER`, made by `service!`) holds the one instance: `start` makes it in the file's `init`, `stop`
//! ends it in `fini`. The hooks and exports of the file reach it through the cell; nothing else holds the state.
//!
//! The crate stays neutral: the SDK calls are behind [`Host`], which the file implements (`dks/src/glue/host.rs`) and a host test implements over fakes.
use crate::engine::{Env, Files, Pending, ProjectService};
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{Provider, Status};
use dkrt_abi::service::SettingsOverlay;

/// What the owner needs from the service file besides the engine: the engine's environment (`Env`, in the UI task), the handles it opens at the start and the table of the
/// settings overlay.
///
/// # Safety
/// All-zero bytes are the initial state of the implementing type: the owner is made in place from zero bytes (`Provider::start_in`; no copy of the state through the stack).
/// `open` runs on that zeroed value.
pub unsafe trait Host: Env {
    /// The client of the write job's file calls: a small `Copy` value that holds what the I/O task needs (`Project::io_step`).
    type Files: Files;

    /// The first step of the file's `init`: open the handles (the listing client, core/ui when the image has it). An error is the status of `init`; a failed `open` leaves
    /// nothing that needs a drop.
    fn open(&mut self) -> Result<(), Status>;

    /// The client of the write job's file calls, read through a raw pointer: the I/O task calls it while the UI task may hold `&mut` to the host state, so it forms no
    /// reference to it.
    ///
    /// # Safety
    /// `this` points to a live host state; the fields that hold the client are not written after `open`.
    unsafe fn files(this: *const Self) -> Self::Files;

    /// A UI pass begins (the clock when the image has no block counter).
    fn begin_pass(&mut self);

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

    /// The tick of the file (the UI task, once per pass): what the hooks recorded, the work in progress, the next piece.
    pub fn tick(&mut self, pending: &Pending) {
        self.host.begin_pass();
        self.engine.tick(&mut self.host, pending);
    }

    /// The write job's step, in the I/O task. It takes a pointer, not `&mut self`: the UI task may be inside `tick` with `&mut` to the whole owner, and the step reads and
    /// writes the writer and the file buffer of the engine and the file client of the host only.
    ///
    /// # Safety
    /// `this` points to a live owner (the cell's instance); the UI task leaves the writer and the buffer alone while a job runs.
    pub unsafe fn io_step(this: *mut Self) -> i32 {
        let mut files = H::files(core::ptr::addr_of!((*this).host));
        ProjectService::io_step_at(core::ptr::addr_of_mut!((*this).engine), &mut files)
    }

    /// The end of the write job, in the UI task.
    pub fn io_done(&mut self, status: i32) {
        self.engine.io_done(status)
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
    fn start(core: &C) -> Result<Project<H>, Status> {
        let mut slot = MaybeUninit::uninit();
        Self::start_in(core, &mut slot)?;
        // SAFETY: `start_in` wrote the value.
        Ok(unsafe { slot.assume_init() })
    }

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

    /// The file's `fini`: the runtime holds no table of this file after it.
    fn stop(&mut self, core: &C) {
        // SAFETY: null takes the table away.
        unsafe { core.settings_overlay_set(core::ptr::null()) };
    }
}
