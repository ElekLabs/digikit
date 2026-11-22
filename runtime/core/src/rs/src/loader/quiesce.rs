//! The teardown of a reload before the old code goes: close every way into the module, then wait until nothing is inside (`Loader::reload_quiesce`), or give the module back
//! its tick and commands (`Loader::reload_abort`).
use super::*;

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// The first step of a reload's teardown, called once per pass until it returns `OK`: close every way into the module's code, then wait until nothing is inside.
    /// The tick stops; its SysEx commands answer `E_BUSY` from now on; its sinks, with the span clock, are cleared (under the interrupt mask); the block count is
    /// noted. It returns `E_BUSY` until no command of the module is in flight and `DRAIN_BLOCKS` blocks have passed (no spinning: the next UI pass asks again).
    /// Nothing is freed and no DSP row is touched, so `reload_abort` can still give the module back its tick and commands. `E_NOT_FOUND`: no such module.
    ///
    /// The channel sinks (`dlog::CHAN`) are cleared and an abort does not set them again: a module that owned one (the debug module's log sink) loses it until the
    /// next boot. The legacy single sink and tick words of `dlog` (`SINK`, `TICK`) are not touched here: they point into the module's code, which stays mapped until
    /// `unload_body` frees it, and `dlog::sink_drop` clears them (masked) before that free, so a task that calls them in between runs code that is still there.
    pub unsafe fn reload_quiesce(&mut self, rec: usize) -> i32 {
        if rec >= MAX_MODULES || records::module(rec).used == 0 {
            return E_NOT_FOUND;
        }
        let owner = rec as u8 + 1;
        if self.torn[rec] & TORN_DRAIN == 0 {
            self.stop_tick_once(rec);
            crate::module_cmds::set_draining(owner, true);
            crate::dlog::release_sinks(rec);
            self.drain_at[rec] = self.p.blocks();
            self.torn[rec] |= TORN_DRAIN;
        }
        if crate::module_cmds::in_flight(owner) != 0 || !crate::reload::drained(&self.p, self.drain_at[rec]) {
            return E_BUSY;
        }
        OK
    }

    /// The teardown of a reload cannot go on (the DSP did not drain in time): if `fini` has not run, the old module gets its tick back and the reload ends with
    /// the old module live. `false` when the old code is gone (`fini` ran): the caller finishes the teardown.
    pub unsafe fn reload_abort(&mut self, rec: usize) -> bool {
        if rec >= MAX_MODULES || self.torn[rec] & TORN_FINI != 0 {
            return false;
        }
        if self.torn[rec] & TORN_TICK != 0 {
            crate::entry::publish_tick(rec, self.saved_tick[rec]);
        }
        crate::module_cmds::set_draining(rec as u8 + 1, false);
        self.torn[rec] = 0;
        true
    }
}

/// The first step of a reload's teardown (`Loader::reload_quiesce`): `OK` when nothing is inside the module any more, `E_BUSY` while the caller must ask again.
pub unsafe fn reload_quiesce(rec: usize) -> i32 {
    loader().reload_quiesce(rec)
}

/// A reload that cannot finish before the old code is gone: give the old module its tick back (`Loader::reload_abort`).
pub unsafe fn reload_abort(rec: usize) -> bool {
    loader().reload_abort(rec)
}
