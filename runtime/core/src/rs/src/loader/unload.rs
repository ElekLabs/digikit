//! The take-down of a record: hooks, ticks, services and the block (`Loader::unload_body`).
use super::*;

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// The ColdFire part of `uninstall`: the hook sites back to stock and the slot words zero.
    pub(super) unsafe fn uninstall_cf(&mut self, rec: usize) {
        self.installer.restore(&self.p, rec as u8);
    }

    /// The DSP part of `uninstall`: the DSP slots zero. False when the DSP does not take the clear.
    pub(super) unsafe fn uninstall_dsp(&mut self, rec: usize) -> bool {
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        if let Some(seq) = self.dsp_install.clear_sequence(rec as u8, area) {
            if !crate::link::lk().run(Owner::Loader, &seq) {
                return false;
            }
        }
        self.dsp_install.restore(&self.p, rec as u8);
        true
    }

    /// Stop the tick of module record `rec` once; the address is kept for a reload that gives up.
    pub(super) unsafe fn stop_tick_once(&mut self, rec: usize) {
        if self.torn[rec] & TORN_TICK == 0 {
            self.saved_tick[rec] = crate::entry::tick_of(rec);
            crate::entry::stop_tick(rec);
            self.torn[rec] |= TORN_TICK;
        }
    }

    /// The start of a service's take-down: no new tick call and no resident query entry. The package manager restores the entries (hook sites, slots) itself, after this and before
    /// it stops the service.
    pub(super) unsafe fn quiesce_service(&mut self, rec: usize) {
        crate::entry::stop_tick(rec);
        self.provs[rec - MAX_MODULES].query_v1 = 0;
    }

    /// The rest of the take-down of record `rec`, after a service's `quiesce_service`.
    pub(super) unsafe fn unload_body(&mut self, rec: usize) -> i32 {
        let module = rec < MAX_MODULES;
        // the listing client and the I/O jobs of a module or a service file go before its code does; a queued job's `done` must not point into freed code
        if !crate::files::retire(rec) { return E_BUSY; }
        if !crate::worker::retire(rec) { return E_BUSY; }
        if !retire_sdk(rec) { return E_BUSY; }
        if module {
            // an intro of this module that has not run `done` yet: `done` must not point into freed code
            crate::intro::discard(rec);
            // the record's next module has opened no settings
            crate::settings_reload::note_unloaded(rec);
        }
        let m: &mut Module = records::module(rec);
        if module {
            // 1. clear the tick (one aligned store) so no new call starts; the UI task, where it runs, is this task. The address is kept for a reload that gives up.
            self.stop_tick_once(rec);
        }
        // DSP rows must be invalidated before fini can mutate/release module state.
        if !dsp::quiesce(rec) { return E_BUSY; }
        // 2. fini, 3. free
        if m.fini != 0 && self.torn[rec] & TORN_FINI == 0 {
            // from here the old code is gone for good: a reload cannot give the tick back
            self.torn[rec] |= TORN_FINI;
            let ctx = crate::entry::ctx_ptr(rec);
            self.m.enter_fini(rec, m.fini, ctx);
        }
        // a sink it set is released with it, with or without a `fini`; nothing may point into its range
        crate::dlog::release_sinks(rec);
        crate::dlog::sink_drop(m.base, m.base.wrapping_add(m.size));
        // the SysEx commands it registered are released with it
        crate::module_cmds::release_cmds(rec as u8 + 1);
        drop_tick_cb(m.base, m.size);
        crate::api::poll_drop_range(m.base, m.size);
        // the unload observers end what they hold for the module (`dkrt_unload_observer_add`): the code and memory are still there, but nothing may call into the module
        if module {
            crate::api::notify_unload(rec, m.base, m.size);
        }
        // the machine row first, then the DSP voice rows, then the range in the DSP area
        if self.torn[rec] & TORN_UNDO == 0 {
            self.torn[rec] |= TORN_UNDO;
            (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
            self.r.remove(rec);
        }
        if module {
            self.reg_block[rec] = (core::ptr::null(), 0);
            let committed = crate::collect::release(rec);
            if committed != 0 {
                self.m.free(DevAddr::new(committed));
            }
        }
        dsp::release_range(rec);
        self.m.free(DevAddr::new(m.base));
        m.used = 0;
        records::set_menu(rec, 0);
        crate::entry::unbind(rec);
        self.torn[rec] = 0;
        if module {
            let b = crate::boot::info();
            b.unloads = b.unloads.wrapping_add(1);
        }
        OK
    }
}

extern "C" {
    /// `core/dsp-link`: clears the per-block callback when it points into [lo, hi). Resolves to 0 when the module is not in the image.
    fn dl_tick_cb_drop();
}

/// After a module's `fini`: no per-block callback of `core/dsp-link` may point into its memory (a module that forgot to clear it).
unsafe fn drop_tick_cb(base: u32, size: u32) {
    let a = core::hint::black_box(dl_tick_cb_drop as unsafe extern "C" fn() as usize);
    if a != 0 {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(a);
        f(base, base.wrapping_add(size));
    }
}

/// The module `rec` is going away (an unload or a rollback): its I/O jobs end without `done`, and a file it was writing is dropped. False: an
/// I/O step of it is running (it is cancelled); the caller keeps the module's memory (`E_BUSY`) until a later call returns true.
pub(crate) unsafe fn retire_sdk(rec: usize) -> bool {
    if !crate::io::retire(rec) {
        return false;
    }
    crate::store_writer::retire(rec as u8 + 1);
    true
}


