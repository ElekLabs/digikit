//! Interface C of docs/ARCHITECTURE.md, the core's side: `RealCore` (what a service calls through `CoreApi`), `FakeCore` (host tests) and the few core functions
//! a service may name directly. The trait and the pure types are in the crate `dkrt-abi`. The services reach the core through this module only
//! (`check:layers` enforces it).
//!
//! `RealCore` is a zero-sized type that delegates to the core functions, so a call through it costs what the direct call cost. A function that a host test must
//! drive takes a generic `C: CoreApi` (no `dyn`: the runtime has no allocator and no vtables); everything else calls the `REAL_CORE` constant.
use crate::{dlog, registry, settings, worker};
use core::ptr::addr_of;
use dkrt_abi::core_api::FX_CLAIMS;
use dkrt_abi::dsp::DspState;
use dkrt_abi::registry::RegState;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

pub use dkrt_abi::core_api::{CoreApi, LinkWriter, JOB_E_CANCELLED, LOG_INFO};

// The C-API glue of the services' imports (`dk_sample_pool_*`): the handle a module holds and the record behind a context.
pub(crate) use crate::entry::{client, give, own_rec};

/// What the services' host tests use of the core's job queue.
#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use crate::worker::{cancel, test_pump, test_reset, TEST_LOCK};
}

/// The claims of the writers that hold the link across polls: set when a submission is accepted, cleared when its result is taken by `link_poll`.
static mut LINK_CLAIM: u8 = 0;

/// The core's own functions, called through `CoreApi`.
#[derive(Clone, Copy)]
pub struct RealCore;

/// The value a function that needs no host test passes to a `CoreApi` call.
pub const REAL_CORE: RealCore = RealCore;

impl CoreApi for RealCore {
    fn device(&self) -> u32 {
        crate::device()
    }
    fn irq_mask(&self) -> u32 {
        crate::irq_mask()
    }
    fn irq_restore(&self, sr: u32) {
        crate::irq_restore(sr)
    }
    unsafe fn dynreg(&self) -> *const u32 {
        registry::info()
    }
    fn reg_state(&self) -> &'static mut RegState {
        registry::rs()
    }
    fn param_first(&self) -> u32 {
        registry::param_first()
    }
    fn param_owners(&self) -> &[u8] {
        unsafe { &(*addr_of!(registry::DKRT_REG)).param_owner }
    }
    unsafe fn write_param_row(&self, inf: *const u32, code: u32, template: u32, slot: u32, min: u32, max: u32, def: u32, name: u32, short: u32) {
        crate::params::write_param_row(inf, code, template, slot, min, max, def, name, short)
    }
    unsafe fn write_fx_param_row(&self, code: u32, template: u32, mirror: u32, min: u32, max: u32, def: u32, name: u32, short: u32) {
        crate::params::write_fx_param_row(code, template, mirror, min, max, def, name, short)
    }
    unsafe fn write_send_param_row(&self, code: u32, class: u32, owner_of: u32, mirror: u32, name: u32, short: u32) {
        crate::params::write_send_param_row(code, class, owner_of, mirror, name, short)
    }
    unsafe fn reset_param_row(&self, code: u32) {
        crate::params::reset_param_row(code)
    }
    unsafe fn make_rep(&self, rep: *mut u8, src: *const u8, upper: bool) -> u32 {
        crate::params::make_rep(rep, src, upper)
    }
    unsafe fn type_resolve(&self, key: u32) -> u32 {
        crate::assign::resolve(key)
    }
    fn type_map_used(&self) -> u32 {
        crate::assign::map_used()
    }
    unsafe fn guard_faults(&self, ty: u32) -> u32 {
        crate::watch::guard_faults(ty)
    }
    unsafe fn guard_disabled(&self, ty: u32) -> bool {
        crate::watch::guard_disabled(ty)
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
    unsafe fn store_probe(&self, path: *const u8, head: Option<&mut [u8; 64]>) -> Result<(u32, [u8; 32]), i32> {
        let f = crate::store::find(path)?;
        if let Some(h) = head {
            crate::store::read_head(&f, h)?;
        }
        Ok((f.size, f.sha))
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
    fn dsp(&self) -> &'static mut DspState {
        crate::dsp::ds()
    }
    unsafe fn dsp_info(&self) -> *const u32 {
        crate::dsp::info()
    }
    unsafe fn fx_info(&self) -> *const u32 {
        crate::dsp::finfo()
    }
    unsafe fn link_word(&self, who: LinkWriter, win: u32, word: u32, value: u32) -> bool {
        let ok = crate::dsp::start_word(win, word, value);
        if ok {
            LINK_CLAIM |= who.claim();
        }
        ok
    }
    unsafe fn link_word_blocking(&self, win: u32, word: u32, value: u32) -> bool {
        crate::dsp::write_word_blocking(win, word, value)
    }
    unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32 {
        let r = crate::dsp::call_send(dst, src, len);
        if r == 0 {
            LINK_CLAIM |= LinkWriter::Sample.claim();
        }
        r
    }
    unsafe fn link_poll(&self, who: LinkWriter) -> Option<bool> {
        let r = crate::dsp::poll_done();
        if r.is_some() {
            LINK_CLAIM &= !who.claim();
        }
        r
    }
    unsafe fn link_idle(&self) -> bool {
        let d = crate::dsp::ds();
        d.phase == crate::dsp::PH_IDLE && d.nwait == 0
    }
    fn link_busy(&self) -> bool {
        unsafe { LINK_CLAIM & FX_CLAIMS != 0 }
    }
    fn link_claimed(&self, who: LinkWriter) -> bool {
        unsafe { LINK_CLAIM & who.claim() != 0 }
    }
}

#[cfg(test)]
pub use fake::{Call, FakeCore};

#[cfg(test)]
mod fake {
    use super::*;
    use core::cell::{Cell, RefCell};
    use std::boxed::Box;
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
        Word { who: LinkWriter, win: u32, word: u32, value: u32 },
        WordBlocking { win: u32, word: u32, value: u32 },
        SampleSend { dst: u32, bytes: Vec<u8> },
        Poll(LinkWriter),
    }

    /// A core that records what services ask of it. `accept` is what the link answers to a submission, `done` what a poll answers. `fx_table` is the
    /// words of `dynfx_info` the fake hands out (first row 0).
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
        pub fx_table: Vec<u32>,
        pub device: u32,
        reg: *mut RegState,
        dsp: *mut DspState,
    }

    impl FakeCore {
        pub fn new() -> Self {
            // zero is every state's valid initial value (the real objects are zeroed statics)
            let reg: &'static mut RegState = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
            let dsp: &'static mut DspState = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
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
                fx_table: std::vec![0; 8],
                device: 1,
                reg,
                dsp,
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
        fn device(&self) -> u32 {
            self.device
        }
        fn irq_mask(&self) -> u32 {
            0
        }
        fn irq_restore(&self, _sr: u32) {}
        unsafe fn dynreg(&self) -> *const u32 {
            core::ptr::null()
        }
        fn reg_state(&self) -> &'static mut RegState {
            unsafe { &mut *self.reg }
        }
        fn param_first(&self) -> u32 {
            self.first
        }
        fn param_owners(&self) -> &[u8] {
            &self.owners
        }
        unsafe fn write_param_row(&self, _inf: *const u32, _c: u32, _t: u32, _slot: u32, _min: u32, _max: u32, _def: u32, _n: u32, _s: u32) {}
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
        unsafe fn type_resolve(&self, key: u32) -> u32 {
            key & 0xff
        }
        fn type_map_used(&self) -> u32 {
            0
        }
        unsafe fn guard_faults(&self, _ty: u32) -> u32 {
            0
        }
        unsafe fn guard_disabled(&self, _ty: u32) -> bool {
            false
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
        unsafe fn store_probe(&self, _path: *const u8, _head: Option<&mut [u8; 64]>) -> Result<(u32, [u8; 32]), i32> {
            Err(dkrt_sdk::E_NOT_FOUND)
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
        fn dsp(&self) -> &'static mut DspState {
            unsafe { &mut *self.dsp }
        }
        unsafe fn dsp_info(&self) -> *const u32 {
            core::ptr::null()
        }
        unsafe fn fx_info(&self) -> *const u32 {
            self.fx_table.as_ptr()
        }
        unsafe fn link_word(&self, who: LinkWriter, win: u32, word: u32, value: u32) -> bool {
            self.rec(Call::Word { who, win, word, value });
            if self.accept.get() {
                self.claim.set(self.claim.get() | who.claim());
            }
            self.accept.get()
        }
        unsafe fn link_word_blocking(&self, win: u32, word: u32, value: u32) -> bool {
            self.rec(Call::WordBlocking { win, word, value });
            true
        }
        unsafe fn link_sample_send(&self, dst: u32, src: *const u8, len: u32) -> i32 {
            self.rec(Call::SampleSend { dst, bytes: core::slice::from_raw_parts(src, len as usize).to_vec() });
            if self.accept.get() {
                self.claim.set(self.claim.get() | LinkWriter::Sample.claim());
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
            self.claim.get() & FX_CLAIMS != 0
        }
        fn link_claimed(&self, who: LinkWriter) -> bool {
            self.claim.get() & who.claim() != 0
        }
    }
}
