//! The service contract suite (docs/plans/extension-model.md 3.5): one generic function, `contract::<S>(&Fixture)`, that every service crate runs in its own
//! `tests/contract.rs`. It proves the reload property of a service on the host: a module that is loaded, unloaded and loaded again leaves the service in the same
//! state, and the loads and unloads of one module do not touch another module's rows.
//!
//! `Lifecycle` runs a module the way the loader does (`loader/mod.rs`, `complete`, `commit_services` and `unload_rec_inner`) over a `FakeCore`: the DSP reservation
//! (`dsp_admit`, `dsp_claim`), `module_begin`, the module's `init` (its builder calls), `module_commit` (the `PREPARE` and `WRITE` passes), the registry walker's
//! check, the `DECLARE` pass, the walker's apply (`apply`, then `apply_record` for each record), and the publication of the DSP rows. A refusal at any step gives the
//! services their `remove` and the DSP clear and release, as the loader's `fail` does; an unload does `dsp_clear`, `remove` and `dsp_release`. The registry walker's own
//! layout check is repeated here for the one service under test (the walker lives in the runtime crate, which this crate sits below).
//!
//! The call lists that stand for the shared fixtures (`tests/fixtures/modules`) are in each service's `tests/contract.rs`: they name the service's own `build::*`, and this
//! crate cannot depend on a service.
use crate::FakeCore;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{Job, DspState, FX_REC, L2_BYTE_BASE, VOICE_V3};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::registry::each;
use dkrt_abi::service::{
    check_step, commit_phase, ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, RecordSink,
    Service,
};
use dkrt_abi::MAX_MODULES;
use dkrt_sdk::*;
use std::collections::BTreeMap;
use std::string::String;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

/// The service statics are shared by the tests of one binary: the suite takes this lock.
static SUITE: Mutex<()> = Mutex::new(());

/// What a module's `init` can do: its builder calls (through the service's `build::*`, with `core` and `ctx`) and `dk_record_add`.
pub struct Init<'a> {
    pub core: &'a FakeCore,
    /// The module record the module loads into.
    pub rec: usize,
    kinds: &'static [(u32, u32)],
    records: Vec<(u32, Vec<u8>)>,
    err: i32,
}

impl Init<'_> {
    /// The context pointer the module passes to its builder calls (`dk_module_ctx *`); the fake core maps any context to the running module.
    pub fn ctx(&self) -> *mut u8 {
        4 as *mut u8
    }

    /// `dk_record_add`: a generic record of one of the service's kinds. The first error is kept and refuses the commit, as the collector does.
    pub fn record(&mut self, kind: u32, body: &[u8]) -> i32 {
        let want = self.kinds.iter().find(|k| k.0 == kind).map(|k| k.1);
        let r = match want {
            Some(l) if l as usize == body.len() + 8 => OK,
            _ => E_FORMAT,
        };
        if r != OK {
            if self.err == 0 {
                self.err = r;
            }
            return r;
        }
        self.records.push((kind, body.to_vec()));
        OK
    }
}

/// The DSP section a module brings: how many voice and effect rows its records claim (the fields of the entries stay 0).
#[derive(Clone, Copy, Default)]
pub struct Dsp {
    pub voices: u32,
    pub fxs: u32,
}

/// One module of a fixture: its header's keys, its `init` and its DSP rows.
pub struct Module {
    pub keys: fn() -> KeysCopy,
    pub init: fn(&mut Init),
    pub dsp: Dsp,
}

/// What a service gives the suite: the profile, the kinds it owns (`[records.kinds]`), the image tables its core has, three modules and a view of its own state.
pub struct Fixture {
    pub name: &'static str,
    /// The device id of the profile (`dkrt_abi::profiles::ALL`).
    pub device: u32,
    /// The record kinds of `[records.kinds]` of the service: (kind, whole record length).
    pub kinds: &'static [(u32, u32)],
    /// The image's tables of the profile (`info_tables`, `fx_table`, `host`), set before the first module loads.
    pub image: fn(&mut FakeCore),
    /// What the service keeps that the firmware memory does not show (its tables of owners and ids), as words: its own query functions.
    pub state: fn(&FakeCore) -> Vec<u32>,
    /// Module A and module B: different keys, the same shape.
    pub a: Module,
    pub b: Module,
    /// A module the service must refuse at its commit (a record it checks, or a builder call it refuses).
    pub bad: Module,
}

/// What a service shows from outside: the firmware words it wrote, the DSP state and the words on the link, its settings, its capacity counts and its own view.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
    mem: BTreeMap<u32, u32>,
    dsp: Vec<u8>,
    /// The last word written to each (window, word) of the link.
    link: BTreeMap<(u32, u32), u32>,
    settings: Vec<(Vec<u8>, Vec<u8>)>,
    caps: Vec<(u8, u32, u32)>,
    state: Vec<u32>,
}

impl Snapshot {
    fn small(&self) -> (Vec<(u8, u32, u32)>, Vec<u32>) {
        (self.caps.clone(), self.state.clone())
    }

    /// What `self` wrote and `other` does not hold any more: every word, link word and DSP state byte that is not 0 in `self` and has another value in `other`.
    /// `other` may have more (rows a module cleared hold the service's free values, rows that were never used hold 0).
    pub fn lost_in(&self, other: &Snapshot) -> String {
        let mut out = String::new();
        let mut n = 0;
        for (k, v) in &self.mem {
            if other.mem.get(k).copied().unwrap_or(0) != *v && n < 8 {
                out.push_str(&std::format!("  mem {k:x}: {v:#x} -> {:#x}\n", other.mem.get(k).copied().unwrap_or(0)));
                n += 1;
            }
        }
        for (k, v) in &self.link {
            if other.link.get(k).copied().unwrap_or(0) != *v && n < 8 {
                out.push_str(&std::format!("  link {k:x?}: {v:#x} -> {:#x}\n", other.link.get(k).copied().unwrap_or(0)));
                n += 1;
            }
        }
        for (i, b) in self.dsp.iter().enumerate() {
            if *b != 0 && other.dsp[i] != *b && n < 8 {
                out.push_str(&std::format!("  dsp state byte {i}: {b:#x} -> {:#x}\n", other.dsp[i]));
                n += 1;
            }
        }
        if self.settings != other.settings {
            out.push_str("  settings differ\n");
        }
        if self.state != other.state {
            out.push_str(&std::format!("  state {:x?} -> {:x?}\n", self.state, other.state));
        }
        out
    }

    /// The first differences between two snapshots, for the failure message.
    pub fn diff(&self, other: &Snapshot) -> String {
        let mut out = String::new();
        fn map<K: Ord + Copy + core::fmt::Debug>(
            out: &mut String,
            what: &str,
            a: &BTreeMap<K, u32>,
            b: &BTreeMap<K, u32>,
        ) {
            let mut n = 0;
            for k in a.keys().chain(b.keys()) {
                let (x, y) = (a.get(k).copied().unwrap_or(0), b.get(k).copied().unwrap_or(0));
                if x != y && n < 8 {
                    out.push_str(&std::format!("  {what} {k:x?}: {x:#x} -> {y:#x}\n"));
                    n += 1;
                }
            }
        }
        map(&mut out, "mem", &self.mem, &other.mem);
        map(&mut out, "link", &self.link, &other.link);
        if self.dsp != other.dsp {
            let at = (0..self.dsp.len()).find(|i| self.dsp[*i] != other.dsp[*i]).unwrap_or(0);
            out.push_str(&std::format!("  dsp state differs from byte {at}\n"));
        }
        if self.settings != other.settings {
            out.push_str(&std::format!(
                "  settings: {} -> {} entries\n",
                self.settings.len(),
                other.settings.len()
            ));
        }
        if self.caps != other.caps {
            out.push_str(&std::format!("  caps {:?} -> {:?}\n", self.caps, other.caps));
        }
        if self.state != other.state {
            out.push_str(&std::format!("  state {:x?} -> {:x?}\n", self.state, other.state));
        }
        out
    }
}

/// A fake core with one service loaded the way the loader loads its modules.
pub struct Lifecycle<'f, S: Service<FakeCore>> {
    pub core: FakeCore,
    f: &'f Fixture,
    _s: core::marker::PhantomData<S>,
}

/// The base address a module's image has in the host runs (any address that has room for the module).
const BASE: u32 = 0x4000;
/// The base of the DSP range: the effect claim computes program counters from it.
const DSP_BASE: u32 = L2_BYTE_BASE + 0x1000;

impl<'f, S: Service<FakeCore>> Lifecycle<'f, S> {
    /// A core of the fixture's profile, and every owner's `remove` once (the service's statics start empty).
    pub fn new(f: &'f Fixture) -> Self {
        let mut core = FakeCore::new();
        core.device = f.device;
        (f.image)(&mut core);
        let l = Lifecycle {
            core,
            f,
            _s: core::marker::PhantomData,
        };
        for owner in 1..=MAX_MODULES as u8 {
            unsafe { S::remove(&l.core, owner) };
        }
        l
    }

    /// The service's own view of its state, for the checks that compare it.
    pub fn snapshot(&self) -> Snapshot {
        let core = &self.core;
        let d = core.dsp();
        let dsp = unsafe {
            core::slice::from_raw_parts(
                d as *const DspState as *const u8,
                core::mem::size_of::<DspState>(),
            )
        }
        .to_vec();
        let mut link = BTreeMap::new();
        for w in core.link.borrow().t.wire.iter() {
            let v = u32::from_le_bytes([w.bytes[0], w.bytes[1], w.bytes[2], w.bytes[3]]);
            link.insert((w.win, w.dst), v);
        }
        link.retain(|_, v| *v != 0);
        let mut settings = core.settings.borrow().clone();
        settings.sort();
        let mut caps = Vec::new();
        for kind in 0..32u8 {
            if let Some((l, u)) = unsafe { S::cap(core, kind) } {
                caps.push((kind, l, u));
            }
        }
        Snapshot {
            mem: core.mem.borrow().iter().filter(|(_, v)| **v != 0).map(|(k, v)| (*k, *v)).collect(),
            dsp,
            link,
            settings,
            caps,
            state: (self.f.state)(core),
        }
    }

    /// `S::declare`: the kinds it names.
    pub fn declared(&self) -> Declare {
        let mut d = Declare::new();
        unsafe { S::declare(&mut d) };
        d
    }

    fn job(&self, m: &Module) -> (Job, Vec<u8>, Vec<u8>) {
        let mut fxs = std::vec![0u8; FX_REC as usize * m.dsp.fxs as usize];
        let mut voices = std::vec![0u8; VOICE_V3 as usize * m.dsp.voices as usize];
        let mut j = Job::empty();
        j.fxs = fxs.as_mut_ptr();
        j.nfx = m.dsp.fxs;
        j.voices = voices.as_mut_ptr();
        j.nvoice = m.dsp.voices;
        j.vstride = VOICE_V3;
        (j, fxs, voices)
    }

    /// The services' part of one module's load, up to the walker's apply; returns the section the walker checked, or the refusal.
    unsafe fn stage(&self, rec: usize, m: &Module, init: &Init) -> Result<Vec<u8>, i32> {
        let core = &self.core;
        let keys = (m.keys)();
        let sink_ctx = |phase: u32, sink: *mut RecordSink, keys: &KeysCopy| ModuleCommitCtx {
            phase,
            base: BASE,
            image_size: 0x4000,
            keys: unsafe { &*(keys as *const KeysCopy) },
            sink,
        };
        let generic = |o: &mut RecordSink| {
            for (kind, body) in &init.records {
                o.head(*kind, 8 + body.len());
                o.bytes(body);
            }
        };
        if init.err != 0 {
            return Err(init.err);
        }
        let mut sink = RecordSink::measure();
        let r = S::module_commit(core, rec, &sink_ctx(commit_phase::PREPARE, &mut sink, &keys));
        if r != OK {
            return Err(r);
        }
        generic(&mut sink);
        let (size, count) = (sink.at, sink.count);
        let mut buf = std::vec![0u8; size];
        let mut out = if size > RecordSink::HEADER {
            RecordSink::over(buf.as_mut_ptr(), size)
        } else {
            RecordSink::over(core::ptr::null_mut(), 0)
        };
        let r = S::module_commit(core, rec, &sink_ctx(commit_phase::WRITE, &mut out, &keys));
        if r != OK {
            return Err(r);
        }
        generic(&mut out);
        if size > RecordSink::HEADER {
            if out.at != size || out.count != count {
                return Err(E_FORMAT); // a service wrote another size than it measured
            }
            for (at, word) in [(0, 0x444b_5247u32), (4, 1), (8, self.f.device), (12, count)] {
                buf[at..at + 4].copy_from_slice(&word.to_be_bytes());
            }
            let r = self.walk_check(&buf);
            if r != OK {
                return Err(r);
            }
        }
        let r = S::module_commit(core, rec, &sink_ctx(commit_phase::DECLARE, &mut out, &keys));
        if r != OK {
            return Err(r);
        }
        if size > RecordSink::HEADER {
            let owner = rec as u8 + 1;
            let c = ApplyCtx {
                owner,
                reg: buf.as_ptr(),
                base: BASE,
            };
            let mut r = S::apply(core, &c);
            if r == OK {
                each(buf.as_ptr(), |kind, b| {
                    if r == OK {
                        r = S::apply_record(core, kind, b, &c);
                    }
                });
            }
            if r != OK {
                return Err(r);
            }
        }
        Ok(buf)
    }

    /// The registry walker's check of a section for this one service: the layout against the kinds' lengths, then the service's check steps in order.
    pub fn walk_check(&self, sec: &[u8]) -> i32 {
        let be = |at: usize| u32::from_be_bytes([sec[at], sec[at + 1], sec[at + 2], sec[at + 3]]);
        if sec.len() < 16 || be(0) != 0x444b_5247 || be(4) != 1 {
            return E_FORMAT;
        }
        if be(8) != self.f.device {
            return E_REG_DEVICE;
        }
        let mut at = 16;
        for _ in 0..be(12) {
            if sec.len() - at < 8 {
                return E_FORMAT;
            }
            let (kind, len) = (be(at), be(at + 4) as usize);
            match self.f.kinds.iter().find(|k| k.0 == kind) {
                Some(k) if k.1 as usize == len && sec.len() - at >= len => at += len,
                _ => return E_FORMAT,
            }
        }
        if at != sec.len() {
            return E_FORMAT;
        }
        let c = CheckCtx {
            reg: sec.as_ptr(),
            image_size: 0x4000,
        };
        for step in 0..check_step::COUNT {
            let r = unsafe { S::check(&self.core, step, &c) };
            if r != OK {
                return r;
            }
        }
        OK
    }

    /// Load module `m` into record `rec`; the loader's steps. A refusal undoes what was done, as the loader's `fail` does, and is returned.
    pub fn load(&self, rec: usize, m: &Module) -> i32 {
        self.load_staged(rec, m).map(|_| OK).unwrap_or_else(|e| e)
    }

    /// `load`, giving the section the walker checked.
    pub fn load_staged(&self, rec: usize, m: &Module) -> Result<Vec<u8>, i32> {
        let core = &self.core;
        let owner = rec as u8 + 1;
        unsafe {
            let (mut job, _fxs, _voices) = self.job(m);
            let dsp = m.dsp.fxs + m.dsp.voices > 0;
            if dsp {
                let r = S::dsp_admit(core, &job);
                if r != OK {
                    return Err(r);
                }
                let mut c = Claim {
                    rec,
                    job: &mut job,
                    base: DSP_BASE,
                    byte_shift: DSP_BASE - L2_BYTE_BASE,
                    state_at: 0,
                };
                S::dsp_claim(core, &mut c);
            }
            core.running.set(owner);
            core.source.set(owner);
            S::module_begin(core, rec);
            let mut init = Init {
                core,
                rec,
                kinds: self.f.kinds,
                records: Vec::new(),
                err: 0,
            };
            (m.init)(&mut init);
            core.running.set(0);
            core.source.set(0);
            match self.stage(rec, m, &init) {
                Ok(sec) => {
                    self.publish(rec);
                    S::on_loaded(core, rec);
                    Ok(sec)
                }
                Err(e) => {
                    S::remove(core, owner);
                    if dsp {
                        S::dsp_clear(core, rec);
                        S::dsp_release(core, rec);
                    }
                    Err(e)
                }
            }
        }
    }

    /// The DSP loader's row publication: the steps of each row go out over the link and mark the row published.
    unsafe fn publish(&self, rec: usize) {
        let core = &self.core;
        let mut step = 0;
        let mut seq = Sequence::new();
        loop {
            let mut want = step;
            let Some(w) = S::dsp_row_step(core, rec, &mut want, true) else {
                break;
            };
            seq.push(w.win, w.word, w.value);
            if let Some(p) = w.published {
                *p = 1;
            }
            if w.last {
                assert!(core.link_run(Owner::Loader, &seq), "the link takes the row");
                seq = Sequence::new();
            }
            step += 1;
        }
    }

    /// Take module record `rec` down: the DSP rows cleared, the services' `remove`, the DSP range released.
    pub fn unload(&self, rec: usize) {
        let core = &self.core;
        unsafe {
            assert!(S::dsp_clear(core, rec), "the DSP rows clear");
            S::remove(core, rec as u8 + 1);
            S::dsp_release(core, rec);
        }
    }

    /// The service's `check` steps over a staged section, for the check that nothing is written: the snapshot before and after.
    pub fn check_only(&self, sec: &[u8]) -> i32 {
        self.walk_check(sec)
    }
}

fn profile(f: &Fixture) -> String {
    dkrt_abi::profiles::ALL
        .iter()
        .find(|p| p.id == f.device)
        .map(|p| {
            String::from_utf8_lossy(&p.name.to_be_bytes())
                .trim_end_matches('\0')
                .to_string()
        })
        .unwrap_or_else(|| format!("device {}", f.device))
}

fn expect(what: &str, f: &Fixture, a: &Snapshot, b: &Snapshot) {
    if a != b {
        panic!(
            "{} ({}): {what}\n{}",
            f.name,
            profile(f),
            a.diff(b)
        );
    }
}

fn locked() -> MutexGuard<'static, ()> {
    SUITE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The six cases of the contract (docs/plans/extension-model.md 3.5) for service `S` on the profile of `f`.
pub fn contract<S: Service<FakeCore>>(f: &Fixture) {
    let _g = locked();
    declares_its_kinds::<S>(f);
    check_writes_nothing::<S>(f);
    reload_equivalence::<S>(f);
    another_module_is_untouched::<S>(f);
    a_refused_commit_leaves_nothing::<S>(f);
    reload_ten_times::<S>(f);
    // leave the statics empty for the next profile
    drop(Lifecycle::<S>::new(f));
}

/// Case 1: `declare` gives exactly the kinds `[records.kinds]` assigns to the service.
fn declares_its_kinds<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert!(
        l.declared().matches_kinds(f.kinds),
        "{}: declare gives other kinds than [records.kinds]",
        f.name
    );
}

/// Case 2: `check` writes nothing, on a section no module has applied and on one a module holds already.
fn check_writes_nothing<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    let calls = l.core.calls().len();
    // a module's section, staged and not applied: load it, take its rows down, and check the section again with the rows in
    let sec = l.load_staged(0, &f.a).unwrap_or_else(|e| panic!("{}: module A is refused with {e}", f.name));
    let before = l.snapshot();
    let calls_before = l.core.calls().len();
    let _ = l.check_only(&sec);
    expect("check wrote to the core (module loaded)", f, &before, &l.snapshot());
    assert_eq!(l.core.calls().len(), calls_before, "{}: check made calls (module loaded)", f.name);
    l.unload(0);
    let before = l.snapshot();
    let calls_before = l.core.calls().len();
    let _ = l.check_only(&sec);
    expect("check wrote to the core (module unloaded)", f, &before, &l.snapshot());
    assert_eq!(l.core.calls().len(), calls_before, "{}: check made calls (module unloaded)", f.name);
    let _ = calls;
}

/// Case 3: the state after `A` is the state after `A`, unload `A`, `A`; the state after `B` is the state after `A`, unload `A`, `B`; the capacity counts and the
/// service's view return to their start after the unload.
fn reload_equivalence<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let after_a = l.snapshot();
    l.unload(0);
    expect_small("after unload A", f, &start, &l.snapshot());
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused after its unload", f.name);
    expect("A, unload A, A differs from A", f, &after_a, &l.snapshot());
    l.unload(0);

    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.b), OK, "{}: module B is refused", f.name);
    let after_b = l.snapshot();
    l.unload(0);
    assert_eq!(l.load(0, &f.a), OK);
    l.unload(0);
    assert_eq!(l.load(0, &f.b), OK, "{}: module B is refused after A", f.name);
    expect("A, unload A, B differs from B", f, &after_b, &l.snapshot());
    l.unload(0);
}

fn expect_small(what: &str, f: &Fixture, a: &Snapshot, b: &Snapshot) {
    if a.small() != b.small() {
        panic!(
            "{} ({}): {what}: capacity counts or the service's view are not back at their start\n  caps {:?} -> {:?}\n  state {:x?} -> {:x?}",
            f.name,
            profile(f),
            a.caps,
            b.caps,
            a.state,
            b.state
        );
    }
}

/// Case 4: with `A` and `B` loaded, `A`'s unload leaves `B` as it was. What `B` alone wrote (its rows, DSP rows and ids) is still there after `B`, `A`, unload `A`
/// (rows that `A` held and cleared may hold the service's free values, which `B` alone leaves at 0); another cycle of `A` changes nothing; and `A`, `B`, unload `A`,
/// unload `B` is the start for the capacity counts and the service's view.
fn another_module_is_untouched<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(1, &f.b), OK, "{}: module B is refused", f.name);
    let b_alone = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused beside B", f.name);
    l.unload(0);
    let lost = b_alone.lost_in(&l.snapshot());
    assert!(lost.is_empty(), "{} ({}): A's unload changed what B holds\n{lost}", f.name, profile(f));
    let once = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused beside B again", f.name);
    l.unload(0);
    expect("a second cycle of A beside B differs from the first", f, &once, &l.snapshot());
    l.unload(1);

    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK);
    assert_eq!(l.load(1, &f.b), OK, "{}: module B is refused beside A", f.name);
    l.unload(0);
    l.unload(1);
    expect_small("A, B, unload A, unload B", f, &start, &l.snapshot());
}

/// Case 5: a refused commit, then the loader's `remove`, leaves the state as it was. A row that was used and released holds the service's free values and a
/// row that never was holds 0, so the state "as it was" is taken after one load and unload of `A` (the rows `bad` claims are the ones `A` released).
fn a_refused_commit_leaves_nothing<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    l.unload(0);
    let before = l.snapshot();
    let r = l.load(0, &f.bad);
    assert_ne!(r, OK, "{}: the module that must be refused is accepted", f.name);
    expect("a refused commit left something", f, &before, &l.snapshot());
    // with a module loaded the refusal of another leaves that module alone
    assert_eq!(l.load(1, &f.b), OK);
    assert_eq!(l.load(0, &f.a), OK);
    l.unload(0);
    let with_b = l.snapshot();
    assert_ne!(l.load(0, &f.bad), OK);
    expect("a refused commit changed a loaded module's state", f, &with_b, &l.snapshot());
    l.unload(1);
}

/// Case 6: case 3 ten times: no table grows and no sticky value drifts.
fn reload_ten_times<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let first = l.snapshot();
    for i in 0..10 {
        l.unload(0);
        assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused at reload {i}", f.name);
        expect(&std::format!("reload {i} of A drifted"), f, &first, &l.snapshot());
    }
    l.unload(0);
    assert_eq!(l.load(0, &f.b), OK);
    let first_b = l.snapshot();
    for i in 0..10 {
        l.unload(0);
        assert_eq!(l.load(0, &f.a), OK);
        l.unload(0);
        assert_eq!(l.load(0, &f.b), OK, "{}: module B is refused at round {i}", f.name);
        expect(&std::format!("round {i} of A and B drifted"), f, &first_b, &l.snapshot());
    }
    l.unload(0);
}
