//! The take-down of a record: hooks, ticks, services and the block (`Loader::unload_body`).
use super::*;

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// The ColdFire part of `uninstall`: the hook sites back to stock and the slot words zero. False when a site that a task below the UI task can be inside cannot be written
    /// yet (the tasks do not wait): nothing was changed.
    pub(super) unsafe fn uninstall_cf(&mut self, rec: usize) -> bool {
        let parks = self.task_parks();
        self.installer.restore_gated(&self.p, rec as u8, &parks.gate(self.live())).is_ok()
    }

    /// The DSP part of `uninstall`: the DSP slots zero. False when the DSP does not take the clear.
    pub(super) unsafe fn uninstall_dsp(&mut self, rec: usize) -> bool {
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        if let Some(seq) = self.dsp_install.clear_sequence(rec as u8, area) {
            if !crate::package::link::lk().run(Owner::Loader, &seq) {
                return false;
            }
        }
        self.dsp_install.restore(&self.p, rec as u8);
        true
    }

    /// Stop the tick of module record `rec` once; the address is kept for a reload that gives up.
    pub(super) unsafe fn stop_tick_once(&mut self, rec: usize) {
        if self.torn[rec] & TORN_TICK == 0 {
            self.saved_tick[rec] = crate::package::entry::tick_of(rec);
            crate::package::entry::stop_tick(rec);
            self.torn[rec] |= TORN_TICK;
        }
    }

    /// The start of a service's take-down: no new tick call and no resident query entry. The package manager restores the entries (hook sites, slots) itself, after this and before
    /// it stops the service.
    pub(super) unsafe fn quiesce_service(&mut self, rec: usize) {
        crate::package::entry::stop_tick(rec);
        self.provs[rec - MAX_MODULES].query_v1 = 0;
    }

    /// The rest of the take-down of record `rec`, after a service's `quiesce_service`.
    pub(super) unsafe fn unload_body(&mut self, rec: usize) -> i32 {
        let module = rec < MAX_MODULES;
        // the listing client and the I/O jobs of a module or a service file go before its code does; a queued job's `done` must not point into freed code
        if !retire_jobs(rec) { return E_BUSY; }
        if module {
            // an intro of this module that has not run `done` yet: `done` must not point into freed code
            crate::package::resident::intro::discard(rec);
            // the record's next module has opened no settings
            crate::package::settings_reload::note_unloaded(rec);
        }
        let m: &mut Module = records::module(rec);
        if module {
            // 1. clear the tick (one aligned store) so no new call starts; the UI task, where it runs, is this task. The address is kept for a reload that gives up.
            self.stop_tick_once(rec);
        }
        // DSP rows must be invalidated before fini can mutate/release module state.
        if !dsp::quiesce(rec) { return E_BUSY; }
        crate::package::module_ui::withdraw(rec);
        // 2. fini, 3. free
        if m.fini != 0 && self.torn[rec] & TORN_FINI == 0 {
            // from here the old code is gone for good: a reload cannot give the tick back
            self.torn[rec] |= TORN_FINI;
            let ctx = crate::package::entry::ctx_ptr(rec);
            self.m.enter_fini(rec, m.fini, ctx);
        }
        drop_registrations(rec, m.base, m.size);
        // the unload observers end what they hold for the module (`dkrt_unload_observer_add`): the code and memory are still there, but nothing may call into the module
        crate::package::resident::api::notify_unload(rec, m.base, m.size);
        // the machine row first, then the DSP voice rows, then the range in the DSP area
        if self.torn[rec] & TORN_UNDO == 0 {
            self.torn[rec] |= TORN_UNDO;
            (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
            self.r.remove(rec);
        }
        if module {
            self.reg_block[rec] = (core::ptr::null(), 0);
            let committed = crate::package::collect::release(rec);
            if committed != 0 {
                self.m.free(DevAddr::new(committed));
            }
        }
        dsp::release_range(rec);
        self.m.free(DevAddr::new(m.base));
        m.used = 0;
        records::set_menu(rec, 0);
        crate::package::entry::unbind(rec);
        self.torn[rec] = 0;
        if module {
            let b = crate::package::resident::boot::info();
            b.unloads = b.unloads.wrapping_add(1);
        }
        OK
    }
}

/// The jobs of record `rec` that run code of its file: the listing client, the worker jobs (`dk_job_post`) and the I/O jobs and file writer. False while one still runs (it is
/// cancelled; ask again). Repeatable. A queued job's `step` and `done` point into the file's block, so this runs before the block is freed: by the unload, by a load that
/// replaces the record, and by `fail`, for every file whose `init` ran (a job posted by `init` stays queued when the commit then refuses).
pub(super) unsafe fn retire_jobs(rec: usize) -> bool {
    // all three run: each one only cancels, and a later call finishes what is still running
    let files = crate::package::files::retire(rec);
    let jobs = crate::package::resident::worker::retire(rec);
    let sdk = retire_sdk(rec);
    files && jobs && sdk
}

/// What the code of record `rec` (range `[base, base + size)`) registered in the resident tables, undone: its sinks, SysEx commands, `core/dsp-link` tick callback, poll
/// functions and unload observers. Nothing may point into the range afterwards. Idempotent; `unload_body` runs it after `fini` and `fail` after a refused load.
pub(super) unsafe fn drop_registrations(rec: usize, base: u32, size: u32) {
    // a sink it set is released with it, with or without a `fini`; nothing may point into its range
    crate::package::resident::dlog::release_sinks(rec);
    crate::package::resident::dlog::sink_drop(base, base.wrapping_add(size));
    // the SysEx commands it registered are released with it
    crate::package::module_cmds::release_cmds(rec as u8 + 1);
    drop_tick_cb(base, size);
    crate::package::resident::api::poll_drop_range(base, size);
}

/// After a module's `fini`: no per-block callback of `core/dsp-link` may point into its memory (a module that forgot to clear it).
unsafe fn drop_tick_cb(base: u32, size: u32) {
    // `core/dsp-link`: clears the per-block callback when it points into [lo, hi); 0 when the module is not in the image.
    let a = crate::package::resident::links::dl_tick_cb_drop();
    if a != 0 {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(a);
        f(base, base.wrapping_add(size));
    }
}

/// The module `rec` is going away (an unload or a rollback): its I/O jobs end without `done`, and a file it was writing is dropped. False: an
/// I/O step of it is running (it is cancelled); the caller keeps the module's memory (`E_BUSY`) until a later call returns true.
pub(crate) unsafe fn retire_sdk(rec: usize) -> bool {
    if !crate::package::resident::io::retire(rec) {
        return false;
    }
    crate::package::resident::store_writer::retire(rec as u8 + 1);
    true
}


