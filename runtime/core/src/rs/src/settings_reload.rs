//! The reload of modules whose settings changed behind their back. A service that changes what a module's settings return (the settings overlay, `settings_overlay.rs`)
//! names the module with `dkc_settings_changed(owner)`; the module re-reads its settings in `init`, so the runtime reloads it (the live job of `reload.rs`).
//!
//! `ReloadQueue` keeps one `ModuleReload` per module record and runs one reload at a time, once per UI pass (`poll`). Only a loaded module that opened its settings
//! (`dk_settings_open` in `init`) is reloaded; one that did not, or that is not loaded, reads nothing it could re-read. All calls run in the UI task.
//!
//! The rules of a reload that does not go through:
//!
//! - the transport plays (`E_RELOAD_PLAYING`): load on stop. The reload waits and is tried again until the transport stops; it counts for no try.
//! - another refusal or a failure (menus of the module, an open upload, safe mode: nothing a retry changes soon): three tries, with a wait between, then the module is dropped.
//! - a change that arrives while the module's job runs is not lost: each change bumps the module's generation and a job that ends well clears the wait only when
//!   the generation is the one it started with.
use core::ptr::addr_of_mut;
use dkrt_sdk::proto::E_STATE;
use dkrt_sdk::{E_BUSY, E_RELOAD_PLAYING, LIVE_RELOAD, OK};

/// A reload that is refused or fails this many times is dropped; the wait between tries is in UI passes.
const TRIES_MAX: u8 = 3;
const RETRY_PASSES: u16 = 120;

/// What the queue keeps for one module record.
#[derive(Clone, Copy)]
pub struct ModuleReload {
    /// The module opened its settings in `init` (cleared when the record is unloaded).
    opened: bool,
    /// The settings changed and the module waits for its reload.
    pending: bool,
    /// Counts the changes: a job clears `pending` only when no change came since it started.
    generation: u16,
    /// The tries that failed since the last change.
    tries: u8,
}

impl ModuleReload {
    const NONE: ModuleReload = ModuleReload { opened: false, pending: false, generation: 0, tries: 0 };
}

/// The live job's state, as far as the queue reads it.
pub struct JobState {
    /// 1 queued or running.
    pub phase: u32,
    pub result: i32,
}

/// What the queue needs from the system: the live job and the module records.
pub trait System {
    fn job(&mut self) -> JobState;
    /// Queue a reload of the module in record `rec`: the status of `dkrt_live`.
    unsafe fn queue_reload(&mut self, rec: usize) -> i32;
    /// The record of the loaded module with id `id`.
    fn rec_of(&self, id: &[u8]) -> Option<usize>;
    /// Does record `rec` carry the module id `id` (loaded or not: the name stays while the module is torn down)?
    fn named(&self, rec: usize, id: &[u8]) -> bool;
    /// Is a module loaded in record `rec`?
    fn loaded(&self, rec: usize) -> bool;
}

pub struct ReloadQueue {
    mods: [ModuleReload; crate::MAX_MODULES],
    /// The record whose reload job is queued or running, and the generation the job started with.
    queued: Option<(usize, u16)>,
    /// UI passes to wait before the next try.
    wait: u16,
}

impl ReloadQueue {
    pub const fn new() -> ReloadQueue {
        ReloadQueue { mods: [ModuleReload::NONE; crate::MAX_MODULES], queued: None, wait: 0 }
    }

    /// The module of record `rec` opened its settings.
    pub fn note_opened(&mut self, rec: usize) {
        if rec < crate::MAX_MODULES {
            self.mods[rec].opened = true;
        }
    }

    /// The module of record `rec` is unloaded (or replaced by another file): the next module in the record has opened nothing, and a wait for the old one is not the new one's.
    /// The reload job itself unloads its record: its own wait stays.
    pub fn note_unloaded(&mut self, rec: usize) {
        if rec >= crate::MAX_MODULES {
            return;
        }
        let m = &mut self.mods[rec];
        m.opened = false;
        if self.queued.map(|q| q.0) != Some(rec) {
            m.pending = false;
            m.tries = 0;
        }
    }

    /// The provider of the changes is gone: nothing waits, nothing is queued.
    pub fn forget_all(&mut self) {
        for m in self.mods.iter_mut() {
            m.pending = false;
            m.tries = 0;
        }
        self.queued = None;
        self.wait = 0;
    }

    /// `dkc_settings_changed`: the settings of module `id` changed. The module waits for a reload when it opened its settings. While its own reload job runs the module has no
    /// open settings between the teardown and the new `init`: the change counts then too.
    pub fn changed<S: System>(&mut self, sys: &S, id: &[u8]) {
        let rec = match sys.rec_of(id) {
            Some(r) if self.mods[r].opened => r,
            Some(_) => return,
            None => match self.queued {
                Some((q, _)) if sys.named(q, id) => q,
                _ => return,
            },
        };
        let m = &mut self.mods[rec];
        m.pending = true;
        m.tries = 0;
        m.generation = m.generation.wrapping_add(1);
    }

    /// Once per UI pass: the end of the running job, else the next reload. One live job at a time.
    pub unsafe fn poll<S: System>(&mut self, sys: &mut S) {
        if !self.mods.iter().any(|m| m.pending) {
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let job = sys.job();
        if let Some((rec, started)) = self.queued {
            if job.phase == 1 {
                return;
            }
            self.queued = None;
            self.ended(rec, started, job.result);
            return;
        }
        if job.phase == 1 {
            return;
        }
        let Some(rec) = self.mods.iter().position(|m| m.pending) else { return };
        if !sys.loaded(rec) {
            self.mods[rec].pending = false;
            return;
        }
        let started = self.mods[rec].generation;
        let r = sys.queue_reload(rec);
        if r == OK {
            self.queued = Some((rec, started));
        } else if r == E_BUSY || r == E_STATE {
            // another live job or a store write: the next pass
        } else {
            self.ended(rec, started, r);
        }
    }

    /// The reload of `rec` that started at generation `started` ended with `result` (a job's, or the refusal of its queueing).
    fn ended(&mut self, rec: usize, started: u16, result: i32) {
        let m = &mut self.mods[rec];
        if result == OK {
            if m.generation == started {
                m.pending = false;
            } else {
                // a change came meanwhile: the new values are not the ones the job loaded
                m.tries = 0;
            }
        } else if result == E_RELOAD_PLAYING {
            // load on stop: wait and try again, whatever the transport takes
            self.wait = RETRY_PASSES;
        } else {
            m.tries += 1;
            if m.tries >= TRIES_MAX {
                m.pending = false;
            }
            self.wait = RETRY_PASSES;
        }
    }
}

static mut QUEUE: ReloadQueue = ReloadQueue::new();

fn queue() -> &'static mut ReloadQueue {
    unsafe { &mut *addr_of_mut!(QUEUE) }
}

/// The system as the runtime is: the live job (`reload.rs`) and the module records.
struct Real;

impl System for Real {
    fn job(&mut self) -> JobState {
        let s = crate::reload::state();
        JobState { phase: s.phase as u32, result: s.result }
    }
    unsafe fn queue_reload(&mut self, rec: usize) -> i32 {
        crate::reload::dkrt_live(LIVE_RELOAD, crate::loader::records::path(rec).as_ptr())
    }
    fn rec_of(&self, id: &[u8]) -> Option<usize> {
        (0..crate::MAX_MODULES).find(|&rec| unsafe { crate::st().modules[rec].used != 0 } && self.named(rec, id))
    }
    fn named(&self, rec: usize, id: &[u8]) -> bool {
        unsafe {
            let m = &crate::st().modules[rec];
            core::slice::from_raw_parts(m.name.as_ptr(), crate::cstr_len(m.name.as_ptr(), 31)) == id
        }
    }
    fn loaded(&self, rec: usize) -> bool {
        unsafe { crate::loader::records::module(rec).used != 0 }
    }
}

/// `dkc_settings_changed`.
pub unsafe fn changed(id: &[u8]) {
    queue().changed(&Real, id)
}

/// The module of record `rec` opened its settings (`dk_settings_open` in `init`).
pub unsafe fn note_opened(rec: usize) {
    queue().note_opened(rec)
}

/// The module of record `rec` is unloaded (`Loader::unload_rec`).
pub unsafe fn note_unloaded(rec: usize) {
    queue().note_unloaded(rec)
}

/// The provider of the changes is gone.
pub unsafe fn forget_all() {
    queue().forget_all()
}

/// Once per UI pass.
pub unsafe fn poll() {
    queue().poll(&mut Real)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// A scripted system: module ids by record (`None`: not loaded), the live job, and the answers of the next queueing.
    struct Fake {
        names: [Option<&'static [u8]>; crate::MAX_MODULES],
        phase: u32,
        result: i32,
        queue_answer: i32,
        queued: Vec<usize>,
    }
    impl Fake {
        fn new() -> Fake {
            Fake { names: [None; crate::MAX_MODULES], phase: 0, result: OK, queue_answer: OK, queued: Vec::new() }
        }
        fn with(mut self, rec: usize, id: &'static [u8]) -> Fake {
            self.names[rec] = Some(id);
            self
        }
    }
    impl System for Fake {
        fn job(&mut self) -> JobState {
            JobState { phase: self.phase, result: self.result }
        }
        unsafe fn queue_reload(&mut self, rec: usize) -> i32 {
            self.queued.push(rec);
            if self.queue_answer == OK {
                self.phase = 1;
            }
            self.queue_answer
        }
        fn rec_of(&self, id: &[u8]) -> Option<usize> {
            self.names.iter().position(|n| *n == Some(id))
        }
        fn named(&self, rec: usize, id: &[u8]) -> bool {
            self.names[rec] == Some(id)
        }
        fn loaded(&self, rec: usize) -> bool {
            self.names[rec].is_some()
        }
    }

    /// The passes `poll` waits after a refusal, run out (the wait counts down on polls while a module waits; the test only needs them gone).
    unsafe fn wait_out(q: &mut ReloadQueue, _f: &mut Fake) {
        q.wait = 0;
    }

    #[test]
    fn only_a_loaded_module_that_opened_its_settings_waits_for_a_reload() {
        let f = Fake::new().with(0, b"test/a").with(1, b"test/b");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        q.changed(&f, b"test/b");
        q.changed(&f, b"test/unknown");
        assert!(q.mods[0].pending && !q.mods[1].pending);
        assert_eq!(q.mods.iter().filter(|m| m.pending).count(), 1);
    }

    #[test]
    fn the_provider_going_away_clears_the_waits_and_the_queued_job() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        unsafe { q.poll(&mut f) };
        assert!(q.queued.is_some());
        q.forget_all();
        assert!(q.queued.is_none() && !q.mods[0].pending);
        // the job that ends now is nobody's
        f.phase = 2;
        unsafe { q.poll(&mut f) };
        assert!(f.queued == [0]);
    }

    #[test]
    fn an_unloaded_record_forgets_that_it_opened_its_settings_and_a_wait_for_the_old_module() {
        let f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        q.note_unloaded(0);
        assert!(!q.mods[0].opened && !q.mods[0].pending);
        // another module loaded into the record has not opened its settings
        q.changed(&f, b"test/a");
        assert!(!q.mods[0].pending);
    }

    #[test]
    fn the_unload_of_the_reload_job_itself_keeps_its_wait() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        unsafe { q.poll(&mut f) };
        q.note_unloaded(0);
        assert!(q.mods[0].pending);
        // between the teardown and the new `init` a change still counts for the module: the record has no loaded module, its name stays
        let g = q.mods[0].generation;
        struct Gone;
        impl System for Gone {
            fn job(&mut self) -> JobState {
                JobState { phase: 1, result: OK }
            }
            unsafe fn queue_reload(&mut self, _: usize) -> i32 {
                OK
            }
            fn rec_of(&self, _: &[u8]) -> Option<usize> {
                None
            }
            fn named(&self, _: usize, id: &[u8]) -> bool {
                id == b"test/a"
            }
            fn loaded(&self, _: usize) -> bool {
                false
            }
        }
        q.changed(&Gone, b"test/a");
        assert_eq!(q.mods[0].generation, g.wrapping_add(1));
    }

    #[test]
    fn a_reload_that_ends_well_clears_the_wait() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        unsafe {
            q.poll(&mut f);
            assert_eq!(f.queued, [0]);
            // the job runs
            q.poll(&mut f);
            f.phase = 2;
            q.poll(&mut f);
        }
        assert!(!q.mods[0].pending && q.queued.is_none());
    }

    #[test]
    fn a_change_while_the_job_runs_is_not_erased_by_its_end() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        unsafe {
            q.poll(&mut f);
            // a second change names the module while its job runs
            q.changed(&f, b"test/a");
            f.phase = 2;
            q.poll(&mut f);
        }
        assert!(q.mods[0].pending, "the new values were not loaded by that job");
        // the next job takes them
        unsafe { q.poll(&mut f) };
        assert_eq!(f.queued, [0, 0]);
    }

    #[test]
    fn a_reload_refused_while_the_transport_plays_waits_for_it_and_counts_for_no_try() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        unsafe {
            // the job ends with the refusal, five times over: more than the tries allowed
            for _ in 0..5 {
                q.poll(&mut f);
                f.phase = 2;
                f.result = E_RELOAD_PLAYING;
                q.poll(&mut f);
                wait_out(&mut q, &mut f);
            }
            assert!(q.mods[0].pending);
            assert_eq!(q.mods[0].tries, 0);
            // the transport stops
            f.result = OK;
            q.poll(&mut f);
            f.phase = 2;
            q.poll(&mut f);
        }
        assert!(!q.mods[0].pending);
        assert_eq!(f.queued.len(), 6);
    }

    #[test]
    fn another_refusal_is_dropped_after_three_tries() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        f.queue_answer = dkrt_sdk::E_RELOAD_MENUS;
        unsafe {
            for _ in 0..3 {
                assert!(q.mods[0].pending);
                q.poll(&mut f);
                wait_out(&mut q, &mut f);
            }
        }
        assert!(!q.mods[0].pending);
        assert_eq!(f.queued.len(), 3);
    }

    #[test]
    fn a_module_that_is_gone_is_not_waited_for() {
        let mut f = Fake::new().with(0, b"test/a");
        let mut q = ReloadQueue::new();
        q.note_opened(0);
        q.changed(&f, b"test/a");
        f.names[0] = None;
        unsafe { q.poll(&mut f) };
        assert!(!q.mods[0].pending && f.queued.is_empty());
    }
}
