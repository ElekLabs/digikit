//! Interface C of docs/ARCHITECTURE.md: what a service (`src/services/*`) calls in the core. A service reaches parameter rows, the module table,
//! settings, the log, the job queue and the DSP link only through `CoreApi`.
//!
//! `RealCore` is a zero-sized type that delegates to the core functions, so a call through it costs what the direct call cost. A function that a host test must
//! drive takes a generic `C: CoreApi` (no `dyn`: the runtime has no allocator and no vtables); everything else calls the `CORE` constant. `FakeCore` (host tests)
//! records the calls in order.
//!
//! The DSP link is one transfer at a time. Two writers hold it across polls: the effect page values (`LinkWriter::FxPage`) and the send amounts (`LinkWriter::FxSend`).
//! `link_busy` says that one of them has a transfer in flight, which the sample track writer checks before it starts one (interim until the R6 transfer queue).
use crate::{dlog, registry, settings, worker};
use core::ptr::addr_of;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// Who starts a transfer on the DSP link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkWriter {
    /// The effect pages' knob and MIX words.
    FxPage,
    /// The send amount words.
    FxSend,
    /// The sample tracks' table writes (the link is shared with the others by `link_idle` and `link_busy`, not by a claim).
    Sample,
}

impl LinkWriter {
    /// The claim bit a writer holds while its transfer is in flight (0: none).
    const fn claim(self) -> u8 {
        match self {
            LinkWriter::FxPage => 1,
            LinkWriter::FxSend => 2,
            LinkWriter::Sample => 0,
        }
    }
}

/// The claims of the writers that hold the link across polls: set when a submission is accepted, cleared when its result is taken by `link_poll`.
static mut LINK_CLAIM: u8 = 0;

pub trait CoreApi {
    // ---- parameter rows (the registry's run-time rows) -------------------------------------------------------------------
    /// The code of the first run-time parameter row; `u32::MAX` without a registry.
    fn param_first(&self) -> u32;
    /// The owner (module record + 1, 0: free) of each run-time parameter row.
    fn param_owners(&self) -> &[u8];
    unsafe fn write_fx_param_row(&self, code: u32, template: u32, mirror: u32, min: u32, max: u32, def: u32, name: u32, short: u32);
    unsafe fn write_send_param_row(&self, code: u32, class: u32, owner_of: u32, mirror: u32, name: u32, short: u32);
    unsafe fn reset_param_row(&self, code: u32);
    /// A firmware string representation of `src` at `rep`; the address of its characters.
    unsafe fn make_rep(&self, rep: *mut u8, src: *const u8, upper: bool) -> u32;

    // ---- the module table ------------------------------------------------------------------------------------------------
    /// The NUL-terminated id of module record `owner - 1` (`owner` is 1 or more).
    unsafe fn module_name(&self, owner: u8) -> *const u8;

    // ---- settings --------------------------------------------------------------------------------------------------------
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32;
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32;

    // ---- log and source --------------------------------------------------------------------------------------------------
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]);
    /// The module whose code runs now (record index + 1; 0 outside module code).
    fn current_source(&self) -> u8;

    // ---- jobs ------------------------------------------------------------------------------------------------------------
    unsafe fn job_post(&self, owner: u8, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32;
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32;

    // ---- the DSP link ----------------------------------------------------------------------------------------------------
    /// Start the write of word `word` of effect row `row` through window 2. `true` when it was accepted.
    unsafe fn link_fx_word(&self, who: LinkWriter, row: usize, word: u32, value: u32) -> bool;
    /// Start a serialized window-zero transfer of `len` bytes at `src` to `dst`: 0 accepted, -2 refused for now, another negative error.
    unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32;
    /// The result of the transfer `who` started: `Some(true)` done, `Some(false)` failed, `None` still running.
    unsafe fn link_poll(&self, who: LinkWriter) -> Option<bool>;
    /// No load and no transfer is in progress on the link.
    unsafe fn link_idle(&self) -> bool;
    /// An effect page value or a send amount transfer is in flight.
    fn link_busy(&self) -> bool;
}

/// The core's own functions, called through `CoreApi`.
#[derive(Clone, Copy)]
pub struct RealCore;

/// The value a function that needs no host test passes to a `CoreApi` call.
pub const REAL_CORE: RealCore = RealCore;

impl CoreApi for RealCore {
    fn param_first(&self) -> u32 {
        registry::param_first()
    }
    fn param_owners(&self) -> &[u8] {
        unsafe { &(*addr_of!(registry::DKRT_REG)).param_owner }
    }
    unsafe fn write_fx_param_row(&self, code: u32, template: u32, mirror: u32, min: u32, max: u32, def: u32, name: u32, short: u32) {
        registry::write_fx_param_row(code, template, mirror, min, max, def, name, short)
    }
    unsafe fn write_send_param_row(&self, code: u32, class: u32, owner_of: u32, mirror: u32, name: u32, short: u32) {
        registry::write_send_param_row(code, class, owner_of, mirror, name, short)
    }
    unsafe fn reset_param_row(&self, code: u32) {
        registry::reset_param_row(code)
    }
    unsafe fn make_rep(&self, rep: *mut u8, src: *const u8, upper: bool) -> u32 {
        registry::make_rep(rep, src, upper)
    }
    unsafe fn module_name(&self, owner: u8) -> *const u8 {
        crate::st().modules[owner as usize - 1].name.as_ptr()
    }
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32 {
        settings::get(key, out)
    }
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32 {
        settings::put(key, value)
    }
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]) {
        dlog::ev(kind, flags, args)
    }
    fn current_source(&self) -> u8 {
        dlog::current_source()
    }
    unsafe fn job_post(&self, owner: u8, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32 {
        worker::post(owner, step, done, ctx)
    }
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32 {
        worker::cancel(owner, id)
    }
    unsafe fn link_fx_word(&self, who: LinkWriter, row: usize, word: u32, value: u32) -> bool {
        let ok = crate::dsp::fx_word_start(row, word, value);
        if ok {
            LINK_CLAIM |= who.claim();
        }
        ok
    }
    unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32 {
        crate::dsp::sample_send(dst, src, len)
    }
    unsafe fn link_poll(&self, who: LinkWriter) -> Option<bool> {
        let r = if who == LinkWriter::Sample { crate::dsp::sample_poll() } else { crate::dsp::fx_word_poll() };
        if r.is_some() {
            LINK_CLAIM &= !who.claim();
        }
        r
    }
    unsafe fn link_idle(&self) -> bool {
        crate::dsp::sample_idle()
    }
    fn link_busy(&self) -> bool {
        unsafe { LINK_CLAIM != 0 }
    }
}

#[cfg(test)]
pub use fake::{Call, FakeCore};

#[cfg(test)]
mod fake {
    use super::*;
    use core::cell::{Cell, RefCell};
    use std::string::String;
    use std::vec::Vec;

    /// One call the service made, in order.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Call {
        FxRow { code: u32, mirror: u32, def: u32 },
        SendRow { code: u32, mirror: u32 },
        ResetRow(u32),
        SettingsPut(Vec<u8>, Vec<u8>),
        Log { kind: u16, args: [u32; 5] },
        JobPost { owner: u8 },
        JobCancel { owner: u8, id: u32 },
        FxWord { who: LinkWriter, row: usize, word: u32, value: u32 },
        SampleSend { dst: u32, bytes: Vec<u8> },
        Poll(LinkWriter),
    }

    /// A core that records what services ask of it. `accept` is what the link answers to a submission, `done` what a poll answers.
    pub struct FakeCore {
        pub calls: RefCell<Vec<Call>>,
        pub first: u32,
        pub owners: Vec<u8>,
        pub source: Cell<u8>,
        pub settings: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
        pub accept: Cell<bool>,
        pub poll: Cell<Option<bool>>,
        pub idle: Cell<bool>,
        pub claim: Cell<u8>,
        pub name: String,
    }

    impl FakeCore {
        pub fn new() -> Self {
            FakeCore {
                calls: RefCell::new(Vec::new()),
                first: 100,
                owners: Vec::new(),
                source: Cell::new(0),
                settings: RefCell::new(Vec::new()),
                accept: Cell::new(true),
                poll: Cell::new(Some(true)),
                idle: Cell::new(true),
                claim: Cell::new(0),
                name: String::from("fake\0"),
            }
        }
        pub fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }
        fn rec(&self, c: Call) {
            self.calls.borrow_mut().push(c);
        }
    }

    impl CoreApi for FakeCore {
        fn param_first(&self) -> u32 {
            self.first
        }
        fn param_owners(&self) -> &[u8] {
            &self.owners
        }
        unsafe fn write_fx_param_row(&self, code: u32, _t: u32, mirror: u32, _min: u32, _max: u32, def: u32, _n: u32, _s: u32) {
            self.rec(Call::FxRow { code, mirror, def })
        }
        unsafe fn write_send_param_row(&self, code: u32, _c: u32, _o: u32, mirror: u32, _n: u32, _s: u32) {
            self.rec(Call::SendRow { code, mirror })
        }
        unsafe fn reset_param_row(&self, code: u32) {
            self.rec(Call::ResetRow(code))
        }
        unsafe fn make_rep(&self, rep: *mut u8, _src: *const u8, _upper: bool) -> u32 {
            rep as u32
        }
        unsafe fn module_name(&self, _owner: u8) -> *const u8 {
            self.name.as_ptr()
        }
        unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32 {
            for (k, v) in self.settings.borrow().iter() {
                if k == key {
                    out[..v.len()].copy_from_slice(v);
                    return v.len() as i32;
                }
            }
            -1
        }
        unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32 {
            self.rec(Call::SettingsPut(key.to_vec(), value.to_vec()));
            0
        }
        unsafe fn log(&self, kind: u16, _flags: u16, args: [u32; 5]) {
            self.rec(Call::Log { kind, args })
        }
        fn current_source(&self) -> u8 {
            self.source.get()
        }
        unsafe fn job_post(&self, owner: u8, _s: Option<JobStepFn>, _d: Option<JobDoneFn>, _c: *mut u8) -> i32 {
            self.rec(Call::JobPost { owner });
            1
        }
        unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32 {
            self.rec(Call::JobCancel { owner, id });
            0
        }
        unsafe fn link_fx_word(&self, who: LinkWriter, row: usize, word: u32, value: u32) -> bool {
            self.rec(Call::FxWord { who, row, word, value });
            if self.accept.get() {
                self.claim.set(self.claim.get() | who.claim());
            }
            self.accept.get()
        }
        unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32 {
            self.rec(Call::SampleSend { dst, bytes: core::slice::from_raw_parts(src, len as usize).to_vec() });
            if self.accept.get() {
                0
            } else {
                -2
            }
        }
        unsafe fn link_poll(&self, who: LinkWriter) -> Option<bool> {
            self.rec(Call::Poll(who));
            let r = self.poll.get();
            if r.is_some() {
                self.claim.set(self.claim.get() & !who.claim());
            }
            r
        }
        unsafe fn link_idle(&self) -> bool {
            self.idle.get()
        }
        fn link_busy(&self) -> bool {
            self.claim.get() != 0
        }
    }
}
