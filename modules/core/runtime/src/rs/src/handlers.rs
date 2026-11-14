//! Interface B of docs/ARCHITECTURE.md: what the core calls on a service, and only what the service declared.
//!
//! A service implements `Service`. The core keeps one fixed list of the in-crate services (`Services`) and walks it through
//! `ServiceList`, a static list of types: no `dyn`, no table in RAM, and a method a service does not declare costs nothing.
//! The core never names a service outside this file. The builder calls and the static table that `module:check` writes are R7
//! (docs/plans/runtime-and-services.md 5.12); here the declaration is the `impl`.
//!
//! What a service declares:
//! - record kinds of the `register` section (`len`), and their `check`, `apply` and `remove` (the registry walker calls them);
//! - the DSP section parts it owns: `dsp_check_*`, `dsp_admit`, `dsp_claim`, `dsp_release`, `dsp_clear`, `dsp_row_step`
//!   (the DSP loader in `dsp/mod.rs` keeps the area, the row table and the state machine);
//! - capacity counts (`caps`), the module behind a fault site (`attribute`), the guard's state of a machine type (`guard_state`);
//! - the declarations of a module's DSP rows (`has_voice`, `declare_voice`, `declare_fx`) and `on_loaded`.
use crate::dsp::{DspSec, Job};
use crate::registry::ApplyArgs;
use dkrt_sdk::*;

/// The steps of the validation of a `register` section, in the order the registry ran them when it was one body. The walker runs
/// each step over the whole list of services before the next, and a service does the steps for its kinds over its records in file
/// order, so the first fault is the one the old body reported.
pub mod check_step {
    /// The most records of each kind.
    pub const COUNTS: u32 = 0;
    /// Records that name another record of the file (click actions).
    pub const ACTIONS: u32 = 1;
    /// Native pages.
    pub const NATIVE: u32 = 2;
    /// Effect records.
    pub const EFFECTS: u32 = 3;
    /// Records the device must support (pages).
    pub const DEVICE: u32 = 4;
    /// The parameter, page and machine records.
    pub const RECORDS: u32 = 5;
    /// Records that need an owner record of the file (a page needs its machine).
    pub const OWNERS: u32 = 6;
    /// Free rows for what the file adds.
    pub const ROWS: u32 = 7;
    pub const COUNT: u32 = 8;
}

/// What the registry walker gives `check`: the `register` section (its layout is validated) and the registry's table.
pub struct CheckCtx {
    pub reg: *const u8,
    pub image_size: u32,
    /// The words of `dynreg_info` (not null).
    pub inf: *const u32,
}

/// What the registry walker gives `apply`: the (checked) section of module record `owner - 1`, loaded at `base`.
pub struct ApplyCtx<'a> {
    pub owner: u8,
    pub reg: *const u8,
    pub base: u32,
    pub inf: *const u32,
    /// The callback arguments the commit set (`registry::set_args`).
    pub args: Option<&'a ApplyArgs>,
}

/// What `dsp::reserve` gives `dsp_claim`: the module's range is placed at `base` (byte address), and the states of its voices and
/// effects follow image and bss in record order (`state_at` moves on).
pub struct Claim<'a> {
    pub rec: usize,
    pub job: &'a Job,
    pub base: u32,
    pub byte_shift: u32,
    pub state_at: u32,
}

/// One word the DSP loader writes to publish a row: window, word index and value. `published` is set once the write is accepted, when
/// from then on the row may have words in the DSP (so a failure must clear them).
pub struct RowWrite {
    pub win: u32,
    pub word: u32,
    pub value: u32,
    pub published: Option<&'static mut u8>,
}

/// What the guard counted for a machine type (`MODULES` reports it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardState {
    pub faults: u32,
    pub disabled: bool,
}

/// The capacity counts the services declare: one (kind, limit, used) entry per kind of `dkrt_capacity`.
pub struct CapSink {
    e: [(u8, u32, u32); 8],
    n: usize,
}

impl CapSink {
    pub const fn new() -> CapSink {
        CapSink { e: [(0, 0, 0); 8], n: 0 }
    }
    /// A kind that two services declare is the sum of their counts.
    pub fn add(&mut self, kind: u8, limit: u32, used: u32) {
        let mut i = 0;
        while i < self.n {
            if self.e[i].0 == kind {
                self.e[i].1 += limit;
                self.e[i].2 += used;
                return;
            }
            i += 1;
        }
        if self.n < self.e.len() {
            self.e[self.n] = (kind, limit, used);
            self.n += 1;
        }
    }
    /// `(limit, used)` of `kind`, if a service declared it.
    pub fn get(&self, kind: u8) -> Option<(u32, u32)> {
        let mut r = None;
        let mut i = 0;
        while i < self.n {
            if self.e[i].0 == kind {
                r = Some((self.e[i].1, self.e[i].2));
            }
            i += 1;
        }
        r
    }
}

/// A service. Every method has a default that declares nothing; a service implements the ones it owns.
#[allow(unused_variables)]
pub trait Service {
    /// The length of a record of `kind` (the whole record, header included) if this service owns the kind.
    fn len(kind: u32) -> Option<u32> {
        None
    }
    /// Whether the registry's table fits this service's state (the walker refuses with `E_NO_REGISTRY` when not).
    unsafe fn fits(inf: *const u32) -> bool {
        true
    }
    /// One step of the validation (`check_step`) for this service's records. Writes nothing.
    unsafe fn check(step: u32, c: &CheckCtx) -> i32 {
        OK
    }
    /// Write this service's rows that the old body wrote before its per-record pass (parameters, actions, pages, machines).
    unsafe fn apply(c: &ApplyCtx) -> i32 {
        OK
    }
    /// Write one record of this service's kinds, in file order after every `apply` (native pages and effect pages interleave as in the
    /// file). A kind this service does not own: OK.
    unsafe fn apply_record(kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        OK
    }
    /// Give back everything module record `owner - 1` filled.
    unsafe fn remove(owner: u8) {}

    /// Add the capacity counts of this service.
    unsafe fn caps(out: &mut CapSink) {}
    /// The module record behind a fault site and the id the DSP reported there (`fault::SITE_*`).
    unsafe fn attribute(site: u32, id: u32) -> Option<usize> {
        None
    }
    /// The guard's counts for machine type `ty`, if this service owns machine types.
    unsafe fn guard_state(ty: u32) -> Option<GuardState> {
        None
    }
    /// Every row of module `rec` is in the DSP.
    unsafe fn on_loaded(rec: usize) {}

    /// Did module `rec` claim a voice row for machine type `ty`?
    unsafe fn has_voice(rec: usize, ty: u32) -> bool {
        false
    }
    /// The ranges and latch mask `init` declared for the voice of type `ty` (`has_voice` is true). `Some(E_*)` or `Some(OK)` if this
    /// service owns the voice.
    unsafe fn declare_voice(rec: usize, ty: u32, rng: [u32; 8], latched: u32) -> Option<i32> {
        None
    }
    /// The route flags and defaults `init` declared for effect record `ordinal` of module `rec`; `Some(false)`: no such effect.
    unsafe fn declare_fx(rec: usize, ordinal: u32, flags: Option<u32>, par: Option<[u32; 8]>) -> Option<bool> {
        None
    }

    /// DSP section: the entries of the tables this service owns (called in list order, after the header is validated).
    unsafe fn dsp_check_sections(sec: &DspSec) -> i32 {
        OK
    }
    /// DSP section: free rows for this service's entries.
    unsafe fn dsp_check_rows(sec: &DspSec) -> i32 {
        OK
    }
    /// DSP section: the machine types of the voices (after every service's rows were checked).
    unsafe fn dsp_check_types(sec: &DspSec) -> i32 {
        OK
    }
    /// Refuse a reservation before anything is reserved (`OK` or an `E_*`).
    unsafe fn dsp_admit(job: &Job) -> i32 {
        OK
    }
    /// Claim this service's rows for module `rec`.
    unsafe fn dsp_claim(c: &mut Claim) {}
    /// Give back what `dsp_claim` took (the rows are cleared).
    unsafe fn dsp_release(rec: usize) {}
    /// Free the rows of module `rec` this service published in the DSP; false when a write failed.
    unsafe fn dsp_clear(rec: usize) -> bool {
        true
    }
    /// The next word to publish for module `rec`: `want` counts the steps since the first row; a row of this service takes its steps off
    /// `want`, and the row that holds the step returns its write. The word is read from the row only when `write` is set (a step in
    /// flight needs to know that a row exists, not what it writes).
    unsafe fn dsp_row_step(rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        None
    }
}

/// A static list of services; `()` is the empty list and `(S, L)` is `S` then `L`.
pub trait ServiceList {
    fn len(kind: u32) -> Option<u32>;
    unsafe fn fits(inf: *const u32) -> bool;
    /// One step of the validation over the services in list order; the first failure.
    unsafe fn check(step: u32, c: &CheckCtx) -> i32;
    unsafe fn apply(c: &ApplyCtx) -> i32;
    unsafe fn apply_record(kind: u32, body: *const u8, c: &ApplyCtx) -> i32;
    /// Last service first.
    unsafe fn remove(owner: u8);
    unsafe fn caps(out: &mut CapSink);
    unsafe fn attribute(site: u32, id: u32) -> Option<usize>;
    unsafe fn guard_state(ty: u32) -> Option<GuardState>;
    unsafe fn on_loaded(rec: usize);
    unsafe fn has_voice(rec: usize, ty: u32) -> bool;
    unsafe fn declare_voice(rec: usize, ty: u32, rng: [u32; 8], latched: u32) -> Option<i32>;
    unsafe fn declare_fx(rec: usize, ordinal: u32, flags: Option<u32>, par: Option<[u32; 8]>) -> Option<bool>;
    unsafe fn dsp_check_sections(sec: &DspSec) -> i32;
    unsafe fn dsp_check_rows(sec: &DspSec) -> i32;
    unsafe fn dsp_check_types(sec: &DspSec) -> i32;
    unsafe fn dsp_admit(job: &Job) -> i32;
    unsafe fn dsp_claim(c: &mut Claim);
    unsafe fn dsp_release(rec: usize);
    unsafe fn dsp_clear(rec: usize) -> bool;
    unsafe fn dsp_row_step(rec: usize, want: &mut u32, write: bool) -> Option<RowWrite>;
}

impl ServiceList for () {
    fn len(_: u32) -> Option<u32> {
        None
    }
    unsafe fn fits(_: *const u32) -> bool {
        true
    }
    unsafe fn check(_: u32, _: &CheckCtx) -> i32 {
        OK
    }
    unsafe fn apply(_: &ApplyCtx) -> i32 {
        OK
    }
    unsafe fn apply_record(_: u32, _: *const u8, _: &ApplyCtx) -> i32 {
        OK
    }
    unsafe fn remove(_: u8) {}
    unsafe fn caps(_: &mut CapSink) {}
    unsafe fn attribute(_: u32, _: u32) -> Option<usize> {
        None
    }
    unsafe fn guard_state(_: u32) -> Option<GuardState> {
        None
    }
    unsafe fn on_loaded(_: usize) {}
    unsafe fn has_voice(_: usize, _: u32) -> bool {
        false
    }
    unsafe fn declare_voice(_: usize, _: u32, _: [u32; 8], _: u32) -> Option<i32> {
        None
    }
    unsafe fn declare_fx(_: usize, _: u32, _: Option<u32>, _: Option<[u32; 8]>) -> Option<bool> {
        None
    }
    unsafe fn dsp_check_sections(_: &DspSec) -> i32 {
        OK
    }
    unsafe fn dsp_check_rows(_: &DspSec) -> i32 {
        OK
    }
    unsafe fn dsp_check_types(_: &DspSec) -> i32 {
        OK
    }
    unsafe fn dsp_admit(_: &Job) -> i32 {
        OK
    }
    unsafe fn dsp_claim(_: &mut Claim) {}
    unsafe fn dsp_release(_: usize) {}
    unsafe fn dsp_clear(_: usize) -> bool {
        true
    }
    unsafe fn dsp_row_step(_: usize, _: &mut u32, _: bool) -> Option<RowWrite> {
        None
    }
}

impl<H: Service, T: ServiceList> ServiceList for (H, T) {
    fn len(kind: u32) -> Option<u32> {
        match H::len(kind) {
            Some(n) => Some(n),
            None => T::len(kind),
        }
    }
    unsafe fn fits(inf: *const u32) -> bool {
        H::fits(inf) && T::fits(inf)
    }
    unsafe fn check(step: u32, c: &CheckCtx) -> i32 {
        let r = H::check(step, c);
        if r != OK {
            return r;
        }
        T::check(step, c)
    }
    unsafe fn apply(c: &ApplyCtx) -> i32 {
        let r = H::apply(c);
        if r != OK {
            return r;
        }
        T::apply(c)
    }
    unsafe fn apply_record(kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        let r = H::apply_record(kind, body, c);
        if r != OK {
            return r;
        }
        T::apply_record(kind, body, c)
    }
    unsafe fn remove(owner: u8) {
        T::remove(owner);
        H::remove(owner);
    }
    unsafe fn caps(out: &mut CapSink) {
        H::caps(out);
        T::caps(out);
    }
    unsafe fn attribute(site: u32, id: u32) -> Option<usize> {
        match H::attribute(site, id) {
            Some(m) => Some(m),
            None => T::attribute(site, id),
        }
    }
    unsafe fn guard_state(ty: u32) -> Option<GuardState> {
        match H::guard_state(ty) {
            Some(g) => Some(g),
            None => T::guard_state(ty),
        }
    }
    unsafe fn on_loaded(rec: usize) {
        H::on_loaded(rec);
        T::on_loaded(rec);
    }
    unsafe fn has_voice(rec: usize, ty: u32) -> bool {
        H::has_voice(rec, ty) || T::has_voice(rec, ty)
    }
    unsafe fn declare_voice(rec: usize, ty: u32, rng: [u32; 8], latched: u32) -> Option<i32> {
        match H::declare_voice(rec, ty, rng, latched) {
            Some(r) => Some(r),
            None => T::declare_voice(rec, ty, rng, latched),
        }
    }
    unsafe fn declare_fx(rec: usize, ordinal: u32, flags: Option<u32>, par: Option<[u32; 8]>) -> Option<bool> {
        match H::declare_fx(rec, ordinal, flags, par) {
            Some(r) => Some(r),
            None => T::declare_fx(rec, ordinal, flags, par),
        }
    }
    unsafe fn dsp_check_sections(sec: &DspSec) -> i32 {
        let r = H::dsp_check_sections(sec);
        if r != OK {
            return r;
        }
        T::dsp_check_sections(sec)
    }
    unsafe fn dsp_check_rows(sec: &DspSec) -> i32 {
        let r = H::dsp_check_rows(sec);
        if r != OK {
            return r;
        }
        T::dsp_check_rows(sec)
    }
    unsafe fn dsp_check_types(sec: &DspSec) -> i32 {
        let r = H::dsp_check_types(sec);
        if r != OK {
            return r;
        }
        T::dsp_check_types(sec)
    }
    unsafe fn dsp_admit(job: &Job) -> i32 {
        let r = H::dsp_admit(job);
        if r != OK {
            return r;
        }
        T::dsp_admit(job)
    }
    unsafe fn dsp_claim(c: &mut Claim) {
        H::dsp_claim(c);
        T::dsp_claim(c);
    }
    unsafe fn dsp_release(rec: usize) {
        H::dsp_release(rec);
        T::dsp_release(rec);
    }
    unsafe fn dsp_clear(rec: usize) -> bool {
        H::dsp_clear(rec) && T::dsp_clear(rec)
    }
    unsafe fn dsp_row_step(rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        match H::dsp_row_step(rec, want, write) {
            Some(w) => Some(w),
            None => T::dsp_row_step(rec, want, write),
        }
    }
}

/// The in-crate services, in the order the core calls them. Samples come first: its DSP-section checks and its track-table claim
/// ran before the voice and effect parts, and the unreserve of its table before theirs. Machines come before fx for the same reason
/// (the registry applies machines, pages and native pages before effect pages; the removal runs the other way round).
pub type Services = (
    crate::services::samples::records::Samples,
    (crate::services::machines::records::Machines, (crate::services::fx::records::Fx, ())),
);

/// The length of a record of `kind`; `E_FORMAT`-worthy kinds are `None`. For the builder lowering (`collect`).
pub fn len(kind: u32) -> Option<u32> {
    <Services as ServiceList>::len(kind)
}

#[cfg(test)]
pub(crate) mod tests;
