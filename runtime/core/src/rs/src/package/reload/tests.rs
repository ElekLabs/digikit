//! Host tests of the live job over a scripted system (`Script`); the cases are in `tests/` by operation.
use super::*;
use dkrt_sdk::*;
use crate::fake_platform::FakePlatform;

// ---- the job over a scripted system ------------------------------------------------------------------------------------------

/// What the fake system answers, and what the job asked of it, in order.
struct Script {
    found: Option<(usize, usize)>,
    /// The index a load is given (`Ops::alloc_index`).
    alloc: Result<usize, i32>,
    refusal: Option<i32>,
    /// `refusal` answers `E_BUSY` this many times first (`u32::MAX`: always).
    refusal_busy: u32,
    check: i32,
    /// `quiesce` answers `E_BUSY` this many times, then `OK` (`u32::MAX`: always busy).
    quiesce_busy: u32,
    /// `unload` answers `E_BUSY` this many times, then `OK`.
    unload_busy: u32,
    /// Whether the old module can still be given back (`fini` has not run).
    can_abort: bool,
    load: i32,
    /// `check` and `load` answer `E_BUSY` this many times first (`u32::MAX`: always).
    check_busy: u32,
    load_busy: u32,
    /// The DSP part answers `None` this many polls, then this result.
    dsp: (u32, i32),
    /// Whether `cancel_dsp` finds a load to take down.
    cancels: bool,
    service: bool,
    torn_seen: bool,
    /// The DSP loader holds a module it could not release.
    retained: bool,
    /// `dsp_progress` moves on every call (a slow load that goes on).
    progressing: bool,
    progress: u32,
    log: Vec<&'static str>,
    phase_on: bool,
    now: Option<u32>,
    /// The block clock never advances.
    stalled: bool,
    /// `menu_built` answers true from this many calls on (`u32::MAX`: never).
    menu_after: u32,
    /// `reload_service` answers `E_BUSY` this many times, then `service_result`.
    service_busy: u32,
    service_result: i32,
    /// The operation numbers handed out, and the one the job runs as now.
    ops_made: u32,
    op_now: u32,
}

impl Script {
    fn new() -> Script {
        Script { found: Some((2, 5)), alloc: Ok(5), refusal: None, refusal_busy: 0, check: OK, quiesce_busy: 0, unload_busy: 0, can_abort: true, load: OK, check_busy: 0, load_busy: 0, dsp: (0, OK), cancels: true, service: false, torn_seen: false, retained: false, progressing: false, progress: 0, log: Vec::new(), phase_on: false, now: Some(0), stalled: false, menu_after: u32::MAX, service_busy: 0, service_result: OK, ops_made: 0, op_now: 0 }
    }
}

impl Ops for Script {
    unsafe fn find(&mut self, _: *const u8) -> Option<(usize, usize)> {
        self.log.push("find");
        self.found
    }
    unsafe fn is_service(&mut self, _: *const u8) -> bool {
        self.service
    }
    unsafe fn dsp_retained(&mut self) -> bool {
        self.retained
    }
    unsafe fn dsp_progress(&mut self) -> u32 {
        if self.progressing {
            self.progress += 1;
        }
        self.progress
    }
    unsafe fn cancel_dsp(&mut self, _: usize, _: i32) -> bool {
        self.log.push("cancel dsp");
        self.cancels
    }
    unsafe fn alloc_index(&mut self, _: *const u8) -> Result<usize, i32> {
        self.log.push("alloc");
        self.alloc
    }
    unsafe fn refusal(&mut self, rec: Option<usize>) -> Option<i32> {
        self.log.push(if rec.is_some() { "refusal" } else { "refusal load" });
        if self.refusal_busy > 0 {
            self.refusal_busy = if self.refusal_busy == u32::MAX { u32::MAX } else { self.refusal_busy - 1 };
            return Some(E_BUSY);
        }
        self.refusal
    }
    unsafe fn check(&mut self, _: u32, _: usize, _: usize, _: *const u8) -> i32 {
        self.log.push("check");
        if self.check_busy > 0 {
            self.check_busy = if self.check_busy == u32::MAX { u32::MAX } else { self.check_busy - 1 };
            return E_BUSY;
        }
        self.check
    }
    fn phase(&mut self, owner: u8) {
        self.phase_on = owner != 0;
        self.log.push(if owner != 0 { "phase on" } else { "phase off" });
    }
    unsafe fn quiesce(&mut self, _: usize) -> i32 {
        self.log.push("quiesce");
        if self.quiesce_busy == 0 {
            OK
        } else {
            if self.quiesce_busy != u32::MAX {
                self.quiesce_busy -= 1;
            }
            E_BUSY
        }
    }
    unsafe fn abort(&mut self, _: usize) -> bool {
        self.log.push("abort");
        self.can_abort
    }
    unsafe fn menu_built(&mut self, _: usize) -> bool {
        if self.menu_after == 0 {
            return true;
        }
        if self.menu_after != u32::MAX {
            self.menu_after -= 1;
        }
        false
    }
    unsafe fn unload(&mut self, _: usize) -> i32 {
        self.log.push("unload");
        if self.unload_busy == 0 {
            OK
        } else {
            self.unload_busy -= 1;
            E_BUSY
        }
    }
    unsafe fn reload_service(&mut self, _: usize, _: *const u8) -> i32 {
        self.log.push("reload service");
        if self.service_busy > 0 {
            self.service_busy = if self.service_busy == u32::MAX { u32::MAX } else { self.service_busy - 1 };
            return E_BUSY;
        }
        self.service_result
    }
    unsafe fn begin_load(&mut self, _: usize) {
        self.log.push("begin load");
    }
    unsafe fn load(&mut self, op: u32, _: usize, k: usize, _: *const u8) -> i32 {
        self.log.push("load");
        if self.load_busy > 0 {
            self.load_busy = if self.load_busy == u32::MAX { u32::MAX } else { self.load_busy - 1 };
            return E_BUSY;
        }
        // the loader chose a record for a load that got as far as a record
        if op == LIVE_LOAD && (self.load == OK || self.load == LOADING) {
            self.found = Some((2, k));
        }
        self.load
    }
    unsafe fn unloaded(&mut self, _: usize) {
        self.log.push("unloaded");
    }
    unsafe fn retire(&mut self, _: usize) {
        self.log.push("retire");
    }
    unsafe fn dsp_result(&mut self, _: usize) -> Option<i32> {
        self.log.push("dsp");
        if self.dsp.0 == 0 {
            Some(self.dsp.1)
        } else {
            self.dsp.0 -= 1;
            None
        }
    }
    unsafe fn on_loaded(&mut self, _: usize) {
        self.log.push("on_loaded");
    }
    unsafe fn finish(&mut self, ok: bool, changed: bool) {
        self.log.push(if ok { "finish ok" } else if changed { "finish torn" } else { "finish failed" });
    }
    fn blocks(&mut self) -> Option<u32> {
        self.now
    }
    unsafe fn next_op(&mut self) -> u32 {
        self.ops_made += 1;
        self.ops_made
    }
    unsafe fn enter_op(&mut self, op: u32) {
        self.op_now = op;
    }
    unsafe fn leave_op(&mut self) {
        self.op_now = 0;
    }
}

/// Run a reload over `s` until it ends (at most 100000 polls); the final state.
fn run(s: &mut Script) -> (u8, i32, u8) {
    run_op(s, LIVE_RELOAD)
}

/// Run a load, an unload or a reload over `s` until it ends (at most 100000 polls); the final state. A load starts from a system that has no module of that path.
fn run_op(s: &mut Script, op: u32) -> (u8, i32, u8) {
    let mut j = Job::new();
    if op == LIVE_LOAD {
        s.found = None;
    }
    unsafe {
        assert_eq!(j.queue(s, op, b"mods/a.dkm\0".as_ptr()), OK);
        let mut n = 0;
        while j.phase == 1 && n < 100_000 {
            j.poll(s);
            if let (Some(t), false) = (s.now.as_mut(), s.stalled) {
                *t += 100;
            }
            n += 1;
        }
    }
    s.torn_seen = j.torn;
    (j.phase, j.result, j.step)
}


/// The end state of a reload that failed after the old module was torn down, in the log of the job: the services were told (`finish torn`, which frees the placeholders,
/// writes the gap rows and rebuilds the list), the phase is off, and the job says the module is not loaded (`torn`).
fn assert_not_loaded(s: &Script, what: &str) {
    assert!(s.torn_seen, "{what}: the job does not say the module is gone");
    assert_eq!(s.log.iter().filter(|l| **l == "finish torn").count(), 1, "{what}");
    assert!(!s.log.contains(&"finish ok") && !s.log.contains(&"on_loaded"), "{what}");
    assert_eq!(s.log.last(), Some(&"phase off"), "{what}");
    assert!(!s.phase_on, "{what}");
}


fn first(s: &Script, name: &str) -> usize {
    s.log.iter().position(|l| *l == name).unwrap_or_else(|| panic!("{name} did not run: {:?}", s.log))
}

fn nothing_changed(s: &Script, what: &str) {
    for changed in ["begin load", "load", "quiesce", "unload", "unloaded", "retire", "finish ok", "finish failed", "finish torn"] {
        assert!(!s.log.contains(&changed), "{what}: {changed} ran: {:?}", s.log);
    }
    assert!(!s.phase_on && !s.torn_seen, "{what}");
}


fn code(p: &FakePlatform, f: Facts) -> Option<i32> {
    unsafe { refusal(p, &f) }
}
fn all() -> Facts {
    Facts {
        safe_mode: true,
        upload_open: true,
        card_busy: true,
        menus: true,
    }
}


mod commands;
mod load;
mod refusals;
mod reload;
mod service;
mod trace;
mod unload;

#[test]
fn a_job_runs_as_its_own_operation_and_leaves_it_at_its_end() {
    let mut s = Script::new();
    unsafe {
        let (mut a, mut b) = (Job::new(), Job::new());
        assert_eq!(a.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        // a second job queued while the first has not run: its number differs, and each poll enters the number of its own job
        assert_eq!(b.queue(&mut s, LIVE_RELOAD, b"mods/b.dkm\0".as_ptr()), OK);
        a.poll(&mut s);
        let in_a = s.op_now;
        b.poll(&mut s);
        let in_b = s.op_now;
        assert_ne!((in_a, in_b), (0, 0));
        assert_ne!(in_a, in_b);
        a.poll(&mut s);
        assert_eq!(s.op_now, in_a);
        let mut n = 0;
        while a.phase == 1 && n < 100_000 {
            a.poll(&mut s);
            if let Some(t) = s.now.as_mut() {
                *t += 100;
            }
            n += 1;
        }
        assert_eq!(a.phase, 2);
        assert_eq!(s.op_now, 0, "the job's end leaves its operation");
    }
}
