//! Host-test fakes of the runtime interfaces, shared by the crate `dkrt` and the service crates (`modules/core/services/*`).
//!
//! `FakeCore` implements `CoreApi` (interface C) and records the calls a service makes. Its link is the real queue (`dkrt-link`) over a model of
//! `core/dsp-link`. It lives in a crate of its own, below the services and the runtime, so a service crate tests against it without naming the runtime.
extern crate std;
pub mod contract;
mod pool;
pub mod routes;
mod storage;
use core::cell::{Cell, RefCell};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::DspState;
use dkrt_abi::fw::Fw;
use dkrt_abi::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
use dkrt_link::fake::{link, FakeLink};
use dkrt_link::Link;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};
use std::boxed::Box;
use std::string::String;
use std::vec::Vec;
pub use storage::FakeStorage;

/// One call the service made, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// A word written to firmware memory by address (`FwMem::wr`): the rows and tables of the image. The fake keeps them (`FwMem::rd` reads them back).
    Wr(u32, u32),
    /// A byte written to firmware memory by address (`FwMem::wr8`).
    Wr8(u32, u8),
    /// A halfword written to firmware memory by address (`FwMem::wr16`, `wr_half`): the low 16 bits of the value.
    Wr16(u32, u16),
    SettingsPut(Vec<u8>, Vec<u8>),
    Log {
        kind: u16,
        flags: u16,
        args: [u32; 5],
    },
    JobPost {
        owner: u8,
    },
    JobCancel {
        owner: u8,
        id: u32,
    },
    Set {
        slot: Slot,
        value: u32,
    },
    /// A call of the image's or the firmware's function at `addr` (`FwMem::call`) with its arguments.
    Fw {
        addr: u32,
        args: Vec<usize>,
    },
}

/// The platform facts of a `FakeCore` (`SampleHost`): the sample files in a `FakeStorage`, whether the pool adapter is in the image and whether the
/// samples are files of the store (a DN2) or of the firmware's file system (a DT2). The interrupt mask is a counter: `masked` says how many masks are open.
pub struct FakeHost {
    pub storage: FakeStorage,
    pub pool: Cell<bool>,
    pub in_store: Cell<bool>,
    pub masked: Cell<u32>,
}

impl FakeHost {
    pub fn new() -> Self {
        FakeHost {
            storage: FakeStorage::new(),
            pool: Cell::new(true),
            in_store: Cell::new(false),
            masked: Cell::new(0),
        }
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    /// Mask interrupts (a counter: `masked` says how many masks are open); the old status register.
    pub fn mask_irq(&self) -> u32 {
        self.masked.set(self.masked.get() + 1);
        0x2700
    }
    pub fn restore_irq(&self, _sr: u32) {
        self.masked.set(self.masked.get() - 1);
    }
}

/// A job a service posted to the fake core's queue (`FakeCore::job_pump`).
struct FakeJob {
    id: u32,
    owner: u8,
    step: Option<JobStepFn>,
    done: Option<JobDoneFn>,
    ctx: *mut u8,
    cancelled: bool,
    /// The status the last step ended with: `done` is called with it at the next pass.
    ended: Option<i32>,
}

/// A core that records what services ask of it. Its link is the real queue over a model of `core/dsp-link` (`link::fake::FakeLink`): the tests read the
/// wire from `link.borrow().t.wire` and let the model's time pass with `pump`.
pub struct FakeCore {
    /// The firmware words written so far, by address.
    pub mem: RefCell<std::collections::BTreeMap<u32, u32>>,
    pub calls: RefCell<Vec<Call>>,
    pub source: Cell<u8>,
    /// The module `CoreApi::module_of` names for any context: record index + 1 of the code that runs (0: none).
    pub running: Cell<u8>,
    /// The selection index of the module being loaded (`CoreApi::boot_index`).
    pub boot_index: Cell<u32>,
    /// What `CoreApi::live_op` answers (0: no reload runs).
    pub live_op: Cell<u32>,
    /// What `CoreApi::keys_of` hands out: (selection index, kind, keys). Set with `set_keys`; no entry: none.
    pub keys: RefCell<Vec<(u32, u32, Vec<u32>)>>,
    /// The bytes `CoreApi::legacy_state` hands out; `legacy_state_clear` zeroes them (and answers `legacy_clear_result`).
    pub legacy: Cell<[u8; 32]>,
    pub legacy_clear_result: Cell<i32>,
    /// What `settings_put` answers (0: stored); a stored value is read back by `settings_get`.
    pub put_result: Cell<i32>,
    pub settings: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
    pub link: RefCell<&'static mut Link<FakeLink>>,
    pub name: String,
    /// The info entries `CoreApi::info` hands out, by name hash: the words of each. Empty: the image has no table.
    pub info_tables: Vec<(u32, Vec<u32>)>,
    pub device: u32,
    /// What a call of a firmware function (`FwMem::call`) answers when `on_call` is not set.
    pub fw_result: Cell<usize>,
    /// Runs at every `FwMem::call`, while the arguments (pointers into the caller's frame) are valid: a test reads what the function would have read, and answers.
    pub on_call: RefCell<Option<Box<dyn FnMut(u32, &[usize]) -> usize>>>,
    /// The platform facts (`CoreApi::host`).
    pub host: FakeHost,
    /// The table a service gave (`CoreApi::settings_overlay_set`), null: none.
    pub settings_overlay: Cell<*const dkrt_abi::service::SettingsOverlay>,
    /// The module ids `CoreApi::settings_changed` named, in order.
    pub settings_changed: RefCell<Vec<Vec<u8>>>,
    /// The live jobs asked for (`CoreApi::live`): the op and the path without its NUL, in order.
    pub live_calls: RefCell<Vec<(u32, Vec<u8>)>>,
    /// What `CoreApi::live` answers; `OK` is a queued job: the state reads phase 1 until a test sets `live_job`.
    pub live_result: Cell<i32>,
    /// The words `CoreApi::live_state` reports: phase, result, step, unloaded, op.
    pub live_job: Cell<[u32; 5]>,
    /// The paths `CoreApi::selection_add` was given, in order, and what it answers.
    pub selection_adds: RefCell<Vec<Vec<u8>>>,
    pub selection_result: Cell<i32>,
    /// The loaded content modules (`CoreApi::module_name` and `module_path`): (id, path), each NUL terminated, by record. Empty: `module_name` answers `name` and no module has a path.
    pub loaded: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
    /// The store files `CoreApi::store_find` searches, by path.
    pub store_files: RefCell<Vec<Vec<u8>>>,
    jobs: RefCell<Vec<FakeJob>>,
    next_job: Cell<u32>,
    dsp: *mut DspState,
}

impl FakeCore {
    /// The module at selection index `at` provides these keys of record kind `kind` (`CoreApi::keys_of`).
    pub fn set_keys(&self, at: u32, kind: u32, keys: &[u32]) {
        let mut k = self.keys.borrow_mut();
        k.retain(|(a, kd, _)| !(*a == at && *kd == kind));
        k.push((at, kind, keys.to_vec()));
    }

    /// The image has the table `dynreg_info` with these words (word 0, the magic, is set here): `dkrt_svc_params::dynreg` finds it through `CoreApi::info`.
    pub fn set_dynreg(&mut self, mut table: Vec<u32>) {
        table[0] = 0x4459_4e52;
        let hash = fnv1a("dynreg_info");
        self.info_tables.retain(|(h, _)| *h != hash);
        self.info_tables.push((hash, table));
    }

    /// The image has the table `dyndsp_info` (the DSP's voice registry and area) with `area_bytes` bytes of area, the first free voice row at `first` and `free` rows
    /// (the magic is set here): the services find it through `CoreApi::info`.
    pub fn set_dyndsp(&mut self, area_bytes: u32, first: u32, free: u32) {
        use dkrt_abi::dsp::{INFO_MAGIC, I_AREA_BYTES, I_DEVICE, I_MAGIC, I_VFIRST, I_VFREE};
        let mut t = std::vec![0u32; 16];
        t[I_MAGIC] = INFO_MAGIC;
        t[I_DEVICE] = self.device;
        t[I_AREA_BYTES] = area_bytes;
        t[I_VFIRST] = first;
        t[I_VFREE] = free;
        let hash = fnv1a("dyndsp_info");
        self.info_tables.retain(|(h, _)| *h != hash);
        self.info_tables.push((hash, t));
    }

    /// The image has the table `dynfx_info` (the effect chain's rows) with the first free row at `first`, `free` rows and the row size of the device: the fx service finds
    /// it through `CoreApi::info`. Set `device` first (the table names it).
    pub fn set_dynfx(&mut self, first: u32, free: u32) {
        use dkrt_abi::dsp::{
            FX_INFO_MAGIC, FX_ROW_WORDS, F_DEVICE, F_FIRST, F_FREE, F_MAGIC, F_WORDS,
        };
        let mut t = std::vec![0u32; 8];
        t[F_MAGIC] = FX_INFO_MAGIC;
        t[F_DEVICE] = self.device;
        t[F_FIRST] = first;
        t[F_FREE] = free;
        t[F_WORDS] = FX_ROW_WORDS;
        let hash = fnv1a("dynfx_info");
        self.info_tables.retain(|(h, _)| *h != hash);
        self.info_tables.push((hash, t));
    }

    pub fn new() -> Self {
        // zero is every state's valid initial value (the real objects are zeroed statics)
        let dsp: &'static mut DspState = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
        FakeCore {
            mem: RefCell::new(std::collections::BTreeMap::new()),
            calls: RefCell::new(Vec::new()),
            source: Cell::new(0),
            running: Cell::new(0),
            boot_index: Cell::new(u32::MAX),
            live_op: Cell::new(0),
            keys: RefCell::new(Vec::new()),
            legacy: Cell::new([0; 32]),
            legacy_clear_result: Cell::new(0),
            put_result: Cell::new(0),
            settings: RefCell::new(Vec::new()),
            link: RefCell::new(Box::leak(Box::new(link()))),
            name: String::from("fake\0"),
            info_tables: Vec::new(),
            device: 0,
            fw_result: Cell::new(0),
            on_call: RefCell::new(None),
            host: FakeHost::new(),
            settings_overlay: Cell::new(core::ptr::null()),
            settings_changed: RefCell::new(Vec::new()),
            live_calls: RefCell::new(Vec::new()),
            live_result: Cell::new(0),
            live_job: Cell::new([0; 5]),
            selection_adds: RefCell::new(Vec::new()),
            selection_result: Cell::new(0),
            loaded: RefCell::new(Vec::new()),
            store_files: RefCell::new(Vec::new()),
            jobs: RefCell::new(Vec::new()),
            next_job: Cell::new(0),
            dsp,
        }
    }
    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
    /// One UI pass and one worker step, as the runtime's job queue runs them: a job whose last step ended, or that was cancelled, gets its `done` (status
    /// or `JOB_E_CANCELLED`) and leaves the queue; then the first job left runs one step (it stays while the step returns 1). A `done` may post a job.
    pub unsafe fn job_pump(&self) {
        let ended: Vec<FakeJob> = {
            let mut jobs = self.jobs.borrow_mut();
            let (out, keep): (Vec<FakeJob>, Vec<FakeJob>) = jobs
                .drain(..)
                .partition(|j| j.ended.is_some() || j.cancelled);
            *jobs = keep;
            out
        };
        for j in ended {
            if let Some(done) = j.done {
                done(
                    j.ctx,
                    j.ended.unwrap_or(dkrt_abi::core_api::JOB_E_CANCELLED),
                );
            }
        }
        let first = self.jobs.borrow().first().map(|j| (j.id, j.step, j.ctx));
        if let Some((id, Some(step), ctx)) = first {
            let r = step(ctx);
            if r != 1 {
                if let Some(j) = self.jobs.borrow_mut().iter_mut().find(|j| j.id == id) {
                    j.ended = Some(r);
                }
            }
        }
    }
    /// The jobs still queued.
    pub fn jobs_queued(&self) -> usize {
        self.jobs.borrow().len()
    }
    fn rec(&self, c: Call) {
        self.calls.borrow_mut().push(c);
    }
    /// The two bytes of `v` at `a` (high byte first) into the words that hold them; one recorded call.
    fn put16(&self, a: u32, v: u32) {
        self.put_byte(a, v >> 8);
        self.put_byte(a + 1, v);
        self.rec(Call::Wr16(a, v as u16));
    }
    /// The low byte of `v` into the word that holds the byte at `a`.
    fn put_byte(&self, a: u32, v: u32) {
        let shift = 8 * (3 - (a & 3));
        let mut m = self.mem.borrow_mut();
        let w = m.entry(a & !3).or_insert(0);
        *w = (*w & !(0xff << shift)) | ((v & 0xff) << shift);
    }
}

impl dkrt_abi::mem::FwMem for FakeCore {
    unsafe fn rd(&self, a: u32) -> u32 {
        self.mem.borrow().get(&a).copied().unwrap_or(0)
    }
    unsafe fn wr(&self, a: u32, v: u32) {
        self.mem.borrow_mut().insert(a, v);
        self.rec(Call::Wr(a, v));
    }
    /// The memory is a map of big-endian words by aligned address: a byte is part of the word at `a & !3`.
    unsafe fn rd8(&self, a: u32) -> u32 {
        (self.rd(a & !3) >> (8 * (3 - (a & 3)))) & 0xff
    }
    unsafe fn wr8(&self, a: u32, v: u32) {
        self.put_byte(a, v);
        self.rec(Call::Wr8(a, v as u8));
    }
    unsafe fn rd16(&self, a: u32) -> i32 {
        ((((self.rd8(a) << 8) | self.rd8(a + 1)) as u16) as i16) as i32
    }
    unsafe fn wr16(&self, a: u32, v: i32) {
        self.put16(a, v as u32);
    }
    unsafe fn wr_half(&self, a: u32, v: u32) {
        self.put16(a, v);
    }
    unsafe fn call(&self, a: u32, args: &[usize]) -> usize {
        if a == 0 {
            return 0;
        }
        self.rec(Call::Fw {
            addr: a,
            args: args.to_vec(),
        });
        match self.on_call.borrow_mut().as_mut() {
            Some(f) => f(a, args),
            None => self.fw_result.get(),
        }
    }
}

impl CoreApi for FakeCore {
    fn boot_index(&self) -> u32 {
        self.boot_index.get()
    }
    fn live_op(&self) -> u32 {
        self.live_op.get()
    }
    fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        match self
            .keys
            .borrow()
            .iter()
            .find(|(a, k, _)| *a == at && *k == kind)
        {
            Some((_, _, v)) => {
                let n = v.len().min(8);
                out[..n].copy_from_slice(&v[..n]);
                n
            }
            None => 0,
        }
    }
    fn legacy_state(&self) -> [u8; 32] {
        self.legacy.get()
    }
    unsafe fn legacy_state_clear(&self) -> i32 {
        let r = self.legacy_clear_result.get();
        if r == 0 {
            self.legacy.set([0; 32]);
        }
        r
    }
    fn device_id(&self) -> u32 {
        self.device
    }
    unsafe fn module_name(&self, owner: u8) -> *const u8 {
        match self.loaded.borrow().get((owner as usize).wrapping_sub(1)) {
            Some((id, _)) => id.as_ptr(),
            None => self.name.as_ptr(),
        }
    }
    unsafe fn module_path(&self, owner: u8) -> *const u8 {
        match self.loaded.borrow().get((owner as usize).wrapping_sub(1)) {
            Some((_, path)) => path.as_ptr(),
            None => core::ptr::null(),
        }
    }
    unsafe fn store_find(&self, dir: &[u8], name: &[u8], out: &mut [u8; 64]) -> i32 {
        for p in self.store_files.borrow().iter() {
            let last = p.rsplit(|c| *c == b'/').next().unwrap_or(p);
            if p.starts_with(dir) && last == name {
                *out = [0; 64];
                out[..p.len()].copy_from_slice(p);
                return p.len() as i32;
            }
        }
        dkrt_sdk::E_NOT_FOUND
    }
    unsafe fn module_of(&self, _ctx: *const u8) -> u8 {
        self.running.get()
    }
    unsafe fn storage_stat(&self, root: u32, path: &[u8]) -> Result<dkrt_abi::storage::Stat, i32> {
        use dkrt_abi::storage::Storage;
        self.host.storage.stat(root, path)
    }
    unsafe fn storage_stat_head(
        &self,
        root: u32,
        path: &[u8],
        head: &mut [u8; 64],
    ) -> Result<(dkrt_abi::storage::Stat, usize), i32> {
        use dkrt_abi::storage::Storage;
        self.host.storage.stat_head(root, path, head)
    }
    fn irq_mask(&self) -> u32 {
        self.host.mask_irq()
    }
    fn irq_restore(&self, sr: u32) {
        self.host.restore_irq(sr)
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
        let r = self.put_result.get();
        if r == 0 {
            let mut s = self.settings.borrow_mut();
            s.retain(|(k, _)| k != key);
            s.push((key.to_vec(), value.to_vec()));
        }
        r
    }
    unsafe fn settings_overlay_set(&self, overlay: *const dkrt_abi::service::SettingsOverlay) {
        self.settings_overlay.set(overlay);
    }
    unsafe fn settings_changed(&self, id: &[u8]) {
        self.settings_changed.borrow_mut().push(id.to_vec());
    }
    unsafe fn live(&self, op: u32, path: *const u8) -> i32 {
        let n = (0..64).take_while(|i| *path.add(*i) != 0).count();
        self.live_calls
            .borrow_mut()
            .push((op, core::slice::from_raw_parts(path, n).to_vec()));
        let r = self.live_result.get();
        if r == 0 {
            self.live_job.set([1, 0, 1, 0, op]);
        }
        r
    }
    unsafe fn selection_add(&self, path: &[u8]) -> i32 {
        self.selection_adds.borrow_mut().push(path.to_vec());
        self.selection_result.get()
    }
    unsafe fn live_state(&self) -> dkrt_sdk::DkrtLive {
        let [phase, result, step, unloaded, op] = self.live_job.get();
        dkrt_sdk::DkrtLive {
            phase,
            result: result as i32,
            step,
            unloaded,
            op,
        }
    }
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]) {
        self.rec(Call::Log { kind, flags, args })
    }
    fn current_source(&self) -> u8 {
        self.source.get()
    }
    unsafe fn job_post(
        &self,
        owner: u8,
        step: Option<JobStepFn>,
        done: Option<JobDoneFn>,
        ctx: *mut u8,
    ) -> i32 {
        self.rec(Call::JobPost { owner });
        let id = self.next_job.get() + 1;
        self.next_job.set(id);
        self.jobs.borrow_mut().push(FakeJob {
            id,
            owner,
            step,
            done,
            ctx,
            cancelled: false,
            ended: None,
        });
        id as i32
    }
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32 {
        self.rec(Call::JobCancel { owner, id });
        match self
            .jobs
            .borrow_mut()
            .iter_mut()
            .find(|j| j.id == id && j.owner == owner)
        {
            Some(j) => {
                j.cancelled = true;
                0
            }
            None => 0,
        }
    }
    fn dsp(&self) -> &'static mut DspState {
        unsafe { &mut *self.dsp }
    }
    unsafe fn info(&self, name_hash: u32) -> *const u32 {
        self.info_tables
            .iter()
            .find(|(h, _)| *h == name_hash)
            .map_or(core::ptr::null(), |(_, w)| w.as_ptr())
    }
    fn link_set(&self, slot: Slot, value: u32) {
        self.rec(Call::Set { slot, value });
        self.link.borrow_mut().set(slot, value)
    }
    fn link_confirmed(&self, slot: Slot) -> Option<u32> {
        self.link.borrow().confirmed(slot)
    }
    fn link_errors(&self) -> u32 {
        self.link.borrow().errors()
    }
    fn link_forget(&self, win: u32, first: u32, count: u32) {
        self.link.borrow_mut().forget(win, first, count)
    }
    unsafe fn link_submit(&self, owner: Owner, stream: &Stream) -> Option<Ticket> {
        self.link.borrow_mut().submit(owner, stream)
    }
    fn link_result(&self, t: Ticket) -> LinkResult {
        self.link.borrow_mut().result(t)
    }
    unsafe fn link_drain(&self, owner: Owner) -> bool {
        self.link.borrow_mut().drain(owner)
    }
    unsafe fn link_run(&self, owner: Owner, seq: &Sequence) -> bool {
        self.link.borrow_mut().run(owner, seq)
    }
}

/// One call a service made on the firmware's parameter page, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum FwCall {
    PageValue {
        view: usize,
        code: u32,
    },
    Object {
        view: usize,
        code: u32,
        track: u32,
    },
    ObjectValue {
        obj: usize,
        code: u32,
    },
    Store {
        obj: usize,
        code: u32,
        raw: i32,
        track: u32,
    },
    ViewTrack {
        track_ref: usize,
    },
    ParamCode {
        view: usize,
        enc: u32,
    },
}

/// A firmware page that records what a service asks of it (`Fw`). The answers are fields: a test sets them with struct update syntax.
pub struct FakeFw {
    /// A mode key (FUNC, a trig or track mode) is held.
    pub held: bool,
    /// `object` finds no parameter object (answers 0).
    pub no_object: bool,
    /// What `page_value` answers.
    pub page_value: i32,
    /// What `object_value` answers.
    pub object_value: i32,
    /// What `view_track` answers.
    pub track: u32,
    /// What `param_code` answers.
    pub param_code: i32,
    pub calls: RefCell<Vec<FwCall>>,
}

impl Default for FakeFw {
    fn default() -> Self {
        FakeFw {
            held: false,
            no_object: false,
            page_value: 0x0500,
            object_value: 0xffff_f800u32 as i32, // a negative halfword with garbage above
            track: 0,
            param_code: -1,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl FakeFw {
    pub fn calls(&self) -> Vec<FwCall> {
        self.calls.borrow().clone()
    }
    fn rec(&self, c: FwCall) {
        self.calls.borrow_mut().push(c);
    }
}

impl Fw for FakeFw {
    fn mode_key_held(&self) -> bool {
        self.held
    }
    fn page_value(&self, view: usize, code: u32) -> i32 {
        self.rec(FwCall::PageValue { view, code });
        self.page_value
    }
    fn object(&self, view: usize, code: u32, track: u32) -> usize {
        self.rec(FwCall::Object { view, code, track });
        if self.no_object {
            0
        } else {
            0x7000
        }
    }
    fn object_value(&self, obj: usize, code: u32) -> i32 {
        self.rec(FwCall::ObjectValue { obj, code });
        self.object_value
    }
    fn store(&self, obj: usize, code: u32, raw: i32, track: u32) {
        self.rec(FwCall::Store {
            obj,
            code,
            raw,
            track,
        });
    }
    fn view_track(&self, track_ref: usize) -> u32 {
        self.rec(FwCall::ViewTrack { track_ref });
        self.track
    }
    fn param_code(&self, view: usize, enc: u32) -> i32 {
        self.rec(FwCall::ParamCode { view, enc });
        self.param_code
    }
}

/// `dkneeds::keys::name_hash`: FNV-1a 32 of a name.
fn fnv1a(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
