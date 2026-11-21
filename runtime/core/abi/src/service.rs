//! Interface B of docs/ARCHITECTURE.md: what the core calls on a service, and only what the service declared.
//!
//! A service implements `Service`. The core keeps one static table of the in-crate services (`ServiceTable` over `ServiceRow`s,
//! generated from `[[service]]` of sdk/sdk.toml into `gen_services.rs`): a row is the service's name, the record kinds it owns
//! (kind and length, from `[records.kinds]`) and one function pointer per method; there is no `dyn` and no table in RAM.
//! The core never names a service outside the generated table (docs/plans/runtime-and-services.md R7).
//!
//! What a service declares:
//! - record kinds of the `register` section (`len`), and their `check`, `apply` and `remove` (the registry walker calls them);
//! - the DSP section parts it owns: `dsp_check_*`, `dsp_admit`, `dsp_claim`, `dsp_release`, `dsp_clear`, `dsp_row_step`
//!   (the DSP loader in `dsp/mod.rs` keeps the area, the row table and the state machine);
//! - capacity counts (`caps`), the module behind a fault site (`attribute`), the DSP diagnostics words it owns (`diag`) and the check of its tables (`table_check`);
//! - `on_loaded` (every row of a module is in the DSP);
//! - the module lifecycle: `module_begin` before a module's `init`, `module_commit` after it (a `PREPARE`, a `WRITE` and a `DECLARE` pass over a `RecordSink`), and `remove`;
//! - selection assignment: `settle` for boot, and preview followed by commit or restore for a live candidate.
use crate::core_api::CoreApi;
use crate::dsp::{DspSec, Job};
use core::ffi::c_void;
use dkrt_sdk::fault::DIAG_WORDS;
use dkrt_sdk::*;

/// The ABI fingerprint of interfaces B and C between the runtime and a service file (`dk_svc_abi`, header word 0x3c of a `.dks`).
///
/// It is the hash of the bytes of the boundary files of this crate (`FILES_HASH`: `service.rs`, `core_api.rs`, `link.rs`, `dsp.rs`, `registry.rs` and
/// `core_imports.rs`, written by `patchkit gen`) with the `size_of` of every `repr(C)` struct that crosses the boundary mixed in at compile time. The
/// sizes make the value depend on the target: the runtime and a service file built for the same device have the same value, a host build another one
/// (64-bit pointers). The runtime reports its value in INFO bytes 41 to 44 and the loader compares it with the word of a service file; 0 in the
/// word means the file uses neither interface. The value is never 0.
pub const SVC_ABI: u32 = svc_abi();

const fn svc_abi() -> u32 {
    use core::mem::size_of;
    let sizes = [
        size_of::<ServiceRow>(),
        size_of::<CheckCtx>(),
        size_of::<ApplyCtx>(),
        size_of::<Claim>(),
        size_of::<RowWrite>(),
        size_of::<ModuleMachines>(),
        size_of::<TableFault>(),
        size_of::<DeclItem>(),
        size_of::<Declare>(),
        size_of::<Job>(),
        size_of::<DspSec>(),
        size_of::<crate::dsp::DspState>(),
        size_of::<crate::link::Slot>(),
        size_of::<crate::link::Owner>(),
        size_of::<crate::link::Ticket>(),
        size_of::<crate::link::LinkResult>(),
        size_of::<crate::link::Step>(),
        size_of::<crate::link::Sequence>(),
        size_of::<crate::link::Stream>(),
        size_of::<DkrtFx>(),
        size_of::<RecordSink>(),
        size_of::<ModuleCommitCtx>(),
        size_of::<SettleCtx>(),
        size_of::<crate::keys::KeysCopy>(),
        size_of::<SettingsOverlay>(),
    ];
    let mut h = crate::gen_svc_abi::FILES_HASH;
    let mut i = 0;
    while i < sizes.len() {
        h = (h ^ sizes[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    if h == 0 {
        1
    } else {
        h
    }
}

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

/// The places in the core's tick where services run, in the order the core calls them (`ServiceList::tick`). A service names the phase of the tick work it owns.
pub mod tick_phase {
    /// After the I/O tick, before the files tick.
    pub const EARLY: u32 = 0;
    /// After the files tick.
    pub const LATE: u32 = 1;
    /// After the late phase, before the settings tick.
    pub const LAST: u32 = 2;
}

/// What the registry walker gives `check`: the `register` section (its layout is validated).
#[repr(C)]
pub struct CheckCtx {
    pub reg: *const u8,
    pub image_size: u32,
}

/// What the registry walker gives `apply`: the (checked) section of module record `owner - 1`, loaded at `base`.
#[repr(C)]
pub struct ApplyCtx {
    pub owner: u8,
    pub reg: *const u8,
    pub base: u32,
}

/// What `dsp::reserve` gives `dsp_claim`: the module's range is placed at `base` (byte address), and the states of its voices and
/// effects follow image and bss in record order (`state_at` moves on).
#[repr(C)]
pub struct Claim<'a> {
    pub rec: usize,
    pub job: &'a Job,
    pub base: u32,
    pub byte_shift: u32,
    pub state_at: u32,
}

/// One word the DSP loader writes to publish a row: window, word index and value. The loader collects the steps of one row into a link `Sequence`.
/// `published` is set once the write is accepted, when from then on the row may have words in the DSP (so a failure must clear them). `last` ends the
/// row (its type or pc word, the one that makes it visible). `undo` is the word that takes a step back when the sequence fails (`has_undo`).
#[repr(C)]
pub struct RowWrite {
    pub win: u32,
    pub word: u32,
    pub value: u32,
    pub published: Option<&'static mut u8>,
    pub last: bool,
    pub has_undo: bool,
    pub undo: u32,
}

/// What the MODULES reply says of the machines of one module (the answer of the export `dk_machines_module_info`): the voice rows it owns (bit mask), its machine type
/// numbers (`nt` of the 8 bytes in `types`), the guard's fault count for those types and whether the guard switched one of them off (0 or 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(C)]
pub struct ModuleMachines {
    pub rows: u32,
    pub nt: u32,
    pub types: [u8; 8],
    pub faults: u32,
    pub guard_off: u32,
}

/// The first mismatch `Service::table_check` found: the module record index, and two numbers that say which row and what it held (the service's own). The core
/// logs them as the numbers of a `TABLE_BAD` event and reads no meaning into them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(C)]
pub struct TableFault {
    pub module: u32,
    pub a: u32,
    pub b: u32,
}

/// The passes of a module commit (`ModuleCommitCtx::phase`), run over every service in table order.
pub mod commit_phase {
    /// Check what the service collected for the module and measure the records it will write (`sink.at` grows). Changes nothing.
    pub const PREPARE: u32 = 0;
    /// Write the records into `sink.buf` (null when no service has records) and count the rows the module uses (`sink.rows`). Changes nothing else.
    pub const WRITE: u32 = 1;
    /// The loader's registry walker has checked the block and found nothing wrong: declare the DSP details of the module's rows (the sink is the written one, not touched).
    /// A refusal of the walker's check leaves everything as it was.
    pub const DECLARE: u32 = 2;
}

/// The bytes of the module's `register` section that the services write at a commit (`ModuleCommitCtx::sink`). The loader hands the block to the registry walker
/// (`check` and `apply` of the services that own the kinds) after the `WRITE` pass. Writing to a sink without a buffer only measures.
#[repr(C)]
pub struct RecordSink {
    pub buf: *mut u8,
    pub cap: usize,
    /// The bytes written or measured so far, the section header included.
    pub at: usize,
    /// The records written or measured so far.
    pub count: u32,
    /// The rows the module uses, as `[uses]` counts them (machine, parameter and page rows), added by the services in the `WRITE` pass (`MODULE_COMMIT` reports them).
    pub rows: [u32; 3],
}

impl RecordSink {
    /// The header of the section: magic, version, device, record count.
    pub const HEADER: usize = 16;

    /// A sink that only measures.
    pub const fn measure() -> RecordSink {
        RecordSink {
            buf: core::ptr::null_mut(),
            cap: 0,
            at: RecordSink::HEADER,
            count: 0,
            rows: [0; 3],
        }
    }
    /// A sink over `cap` bytes at `buf` (null: measures).
    pub const fn over(buf: *mut u8, cap: usize) -> RecordSink {
        RecordSink {
            buf,
            cap,
            at: RecordSink::HEADER,
            count: 0,
            rows: [0; 3],
        }
    }
    pub fn p32(&mut self, v: u32) {
        if !self.buf.is_null() && self.at + 4 <= self.cap {
            unsafe { crate::mem::wr32(self.buf.add(self.at), v) };
        }
        self.at += 4;
    }
    /// `s` padded with NUL to `field` bytes.
    pub fn text(&mut self, s: &[u8], field: usize) {
        let mut i = 0;
        while i < field {
            if !self.buf.is_null() && self.at + i < self.cap {
                unsafe { *self.buf.add(self.at + i) = if i < s.len() { s[i] } else { 0 } };
            }
            i += 1;
        }
        self.at += field;
    }
    pub fn bytes(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            if !self.buf.is_null() && self.at + i < self.cap {
                unsafe { *self.buf.add(self.at + i) = s[i] };
            }
            i += 1;
        }
        self.at += s.len();
    }
    /// The head of a record of `kind` whose whole length (head included) is `len`.
    pub fn head(&mut self, kind: u32, len: usize) {
        self.p32(kind);
        self.p32(len as u32);
        self.count += 1;
    }
}

/// What the loader gives `module_commit`: where the module is loaded, its keys table and the sink for its records.
#[repr(C)]
pub struct ModuleCommitCtx<'a> {
    /// A `commit_phase`.
    pub phase: u32,
    /// The address the module's image was loaded at, and its size (callback addresses become image offsets).
    pub base: u32,
    pub image_size: u32,
    /// The module's header table: `[uses]` rows, machine keys and effect ids.
    pub keys: &'a crate::keys::KeysCopy,
    pub sink: *mut RecordSink,
}

/// The selection being numbered. During this callback all services query the same
/// typed header keys through `CoreApi::keys_of`. Holes and refused modules expose no keys.
#[repr(C)]
pub struct SettleCtx {
    pub count: usize,
    phase: SettlePhase,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
enum SettlePhase {
    Boot,
    Preview,
    Restore,
    Commit,
}

impl SettleCtx {
    pub const fn new() -> Self {
        Self {
            count: 0,
            phase: SettlePhase::Boot,
        }
    }
    pub const fn preview(count: usize) -> Self {
        Self {
            count,
            phase: SettlePhase::Preview,
        }
    }
    pub const fn restore(count: usize) -> Self {
        Self {
            count,
            phase: SettlePhase::Restore,
        }
    }
    pub const fn commit(count: usize) -> Self {
        Self {
            count,
            phase: SettlePhase::Commit,
        }
    }
    /// Visit a service's keys in selection order, with the owning module's selection index.
    pub fn for_each_key<C: CoreApi>(&self, core: &C, kind: u32, mut visit: impl FnMut(u32, u32)) {
        let mut keys = [0; 8];
        for at in 0..self.count.min(dkrt_sdk::SEL_MAX) {
            let count = core.keys_of(at as u32, kind, &mut keys);
            for &key in &keys[..count] {
                visit(at as u32, key);
            }
        }
    }
    pub fn is_preview(&self) -> bool {
        self.phase == SettlePhase::Preview
    }
    pub fn is_restore(&self) -> bool {
        self.phase == SettlePhase::Restore
    }
    pub fn is_commit(&self) -> bool {
        self.phase == SettlePhase::Commit
    }
}

/// One declaration of `Declare`: (table tag, first word, second word, third word); an unused word is 0.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DeclItem {
    pub tag: u16,
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl DeclItem {
    pub const NONE: DeclItem = DeclItem {
        tag: 0,
        a: 0,
        b: 0,
        c: 0,
    };
}

/// What a service declares at its init (declare-check-commit, docs/plans/runtime-and-services.md R7): the record kinds it owns, the slots and hook
/// sites it takes, the functions it exports and the resources it adds. The items have the shape of the header table's entries of a service module
/// (`KINDS`, `SLOTS`, `HOOKS`, `EXPORTS`, `RESOURCES`: tags 4 to 8 of packages/dkneeds/src/keys.rs), so the runtime compares them with the table
/// (the generated static of an in-crate service, the header table of a module) and a difference means the service FAILED.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Declare {
    /// (table tag, first word, second word, third word): `kind(kind, len)` (and the handler entry of a module), `slot(id, flags, entry)`, `hook(site, entry)`,
    /// `export(name hash, entry)`, `resource(id, limit)`; an unused word is 0.
    pub items: [DeclItem; Declare::MAX],
    pub n: usize,
    /// More declarations than `MAX` were made.
    pub full: bool,
}

impl Declare {
    /// Most declarations of one `init`: the machines service takes 13 hooks, 6 kinds, a slot, its builder exports and its row.
    pub const MAX: usize = 64;
    pub const KIND: u16 = 4;
    pub const SLOT: u16 = 5;
    pub const HOOK: u16 = 6;
    pub const EXPORT: u16 = 7;
    pub const RESOURCE: u16 = 8;
    /// The `ServiceRow` a service file declares (`dk_service_declare_row`): the row's address, low word in `a` and (a 64-bit host) high word in `b`. The header
    /// table has no such entry: `same_as` leaves it out when it compares the declarations with the table.
    pub const ROW: u16 = 9;

    pub const fn new() -> Declare {
        Declare {
            items: [DeclItem::NONE; Declare::MAX],
            n: 0,
            full: false,
        }
    }
    /// One more item; `full` is set when there is no room.
    pub fn add(&mut self, tag: u16, a: u32, b: u32, c: u32) {
        if self.n < Declare::MAX {
            self.items[self.n] = DeclItem { tag, a, b, c };
            self.n += 1;
        } else {
            self.full = true;
        }
    }
    /// A record kind the service owns, with the length of its record (the 8-byte header included).
    pub fn kind(&mut self, kind: u32, len: u32) {
        self.add(Declare::KIND, kind, len, 0);
    }
    pub fn slot(&mut self, slot: u32, flags: u32, entry: u32) {
        self.add(Declare::SLOT, slot, flags, entry);
    }
    pub fn hook(&mut self, site: u32, entry: u32) {
        self.add(Declare::HOOK, site, entry, 0);
    }
    pub fn export(&mut self, name_hash: u32, entry: u32) {
        self.add(Declare::EXPORT, name_hash, entry, 0);
    }
    pub fn resource(&mut self, id: u32, limit: u32) {
        self.add(Declare::RESOURCE, id, limit, 0);
    }
    /// The service file's row.
    pub fn row(&mut self, row: *const ServiceRow) {
        let at = row as usize as u64;
        self.add(Declare::ROW, at as u32, (at >> 32) as u32, 0);
    }
    /// The row of the `ROW` item, if one was declared.
    pub fn row_ptr(&self) -> Option<*const ServiceRow> {
        let mut i = 0;
        while i < self.n {
            let DeclItem { tag, a, b, .. } = self.items[i];
            if tag == Declare::ROW {
                return Some((((b as u64) << 32 | a as u64) as usize) as *const ServiceRow);
            }
            i += 1;
        }
        None
    }
    /// Whether `other` holds the same items (in any order): what a module declared in `init` against what its header table says.
    ///
    /// The `ROW` item of `self` (a service file's own row) has no entry in a header table and is left out.
    pub fn same_as(&self, other: &Declare) -> bool {
        let mut rows = 0;
        let mut r = 0;
        while r < self.n {
            if self.items[r].tag == Declare::ROW {
                rows += 1;
            }
            r += 1;
        }
        if self.full || other.full || self.n - rows != other.n {
            return false;
        }
        let has = |d: &Declare, x: DeclItem| {
            let mut k = 0;
            while k < d.n {
                if d.items[k] == x {
                    return true;
                }
                k += 1;
            }
            false
        };
        let mut i = 0;
        while i < self.n {
            if self.items[i].tag != Declare::ROW && !has(other, self.items[i]) {
                return false;
            }
            i += 1;
        }
        i = 0;
        while i < other.n {
            if !has(self, other.items[i]) {
                return false;
            }
            i += 1;
        }
        true
    }
    /// Whether the declarations are exactly the record kinds `kinds` (kind, length) and nothing else, in any order.
    pub fn matches_kinds(&self, kinds: &[(u32, u32)]) -> bool {
        if self.full || self.n != kinds.len() {
            return false;
        }
        let mut i = 0;
        while i < self.n {
            let DeclItem { tag, a, b, .. } = self.items[i];
            if tag != Declare::KIND {
                return false;
            }
            let mut found = false;
            let mut k = 0;
            while k < kinds.len() {
                if kinds[k].0 == a && kinds[k].1 == b {
                    found = true;
                }
                k += 1;
            }
            if !found {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// One refusal of a set of records, from `Service::validate_records`. The same value serves the load path (`code` is what the loader returns) and a build-time
/// check (`module:check` prints it): one rule, two callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The record kind of the records (`[records.kinds]` of sdk/sdk.toml).
    pub kind: u32,
    /// The position of the refused record among the records of `kind`, from 0; none for a fault of the whole set (its size).
    pub index: Option<u32>,
    /// The earlier record this one clashes with (same name, same key), if any.
    pub with: Option<u32>,
    /// The error code the load path returns (`E_FORMAT`, `E_TYPE_USED`, ...).
    pub code: i32,
    /// What is wrong, a short sentence fragment.
    pub what: &'static str,
}

impl Problem {
    /// A fault of record `index`.
    pub const fn record(kind: u32, index: usize, code: i32, what: &'static str) -> Problem {
        Problem {
            kind,
            index: Some(index as u32),
            with: None,
            code,
            what,
        }
    }
    /// A fault of record `index` that clashes with the earlier record `with`.
    pub const fn clash(
        kind: u32,
        index: usize,
        with: usize,
        code: i32,
        what: &'static str,
    ) -> Problem {
        Problem {
            kind,
            index: Some(index as u32),
            with: Some(with as u32),
            code,
            what,
        }
    }
    /// A fault of the whole set (too many or too few records).
    pub const fn set(kind: u32, code: i32, what: &'static str) -> Problem {
        Problem {
            kind,
            index: None,
            with: None,
            code,
            what,
        }
    }
}

impl core::fmt::Display for Problem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "record kind {}", self.kind)?;
        if let Some(i) = self.index {
            write!(f, ", record {i}")?;
        }
        write!(f, ": {}", self.what)?;
        if let Some(w) = self.with {
            write!(f, " (record {w})")?;
        }
        Ok(())
    }
}

/// The `validate_records` of a service that owns no record kind, or whose records need no rule beyond the layout the registry walker checks: it accepts.
/// Write it in the `impl`: `dkrt_abi::accepts_records!();`.
#[macro_export]
macro_rules! accepts_records {
    () => {
        fn validate_records(
            _kind: u32,
            _bodies: &[&[u8]],
            _report: &mut dyn FnMut($crate::service::Problem),
        ) {
        }
    };
}

/// The `teardown_refusal` of a service that has nothing a reload of a module must wait for: it never refuses. Write it in the `impl`: `dkrt_abi::accepts_teardown!();`.
#[macro_export]
macro_rules! accepts_teardown {
    () => {
        unsafe fn teardown_refusal(_core: &C, _owner: u8) -> i32 {
            0
        }
    };
}

/// How a service file names the firmware. 2: through the port file (docs/plans/port-file.md): `dk_fw_`, `dk_exit_`, `dk_tail_`, `dk_stock_` and `dk_ret_` imports that the
/// loader resolves from it, and raw `HOOKS` entries whose sites it lists; the file has a hard `NEED` of `port` and no `BIND` or `FW` item. 1 was the file bound to one
/// firmware by the port file. The constant is part of the boundary files, so a change of it changes `SVC_ABI` and the runtime refuses the older files (`E_SVC_ABI`).
pub const FIRMWARE_REFERENCES: u32 = 2;

/// The device mask in the flags of a `SLOTS` item of a service file (`dkneeds::keys::slot_for_device`): bits 16 to 23, bit `id - 1` for the device with runtime id `id`, none for
/// every device. The loader skips an item for another device, so one file serves every device. The constant is part of the boundary files: a runtime without the mask would
/// refuse such an item, so the change of this layout changes `SVC_ABI` and the install refuses the mismatch before boot.
pub const SLOT_DEVICES_MASK: u32 = dkneeds::keys::SLOT_DEVICES_MASK;

/// A service. Every method but `validate_records` and `teardown_refusal` has a default that declares nothing; a service implements the ones it owns.
/// The core is passed by reference to every method that may touch it (`declare` reads nothing but its argument); a service
/// that a host test drives is generic over `C: CoreApi` and the test passes a `FakeCore`.
#[allow(unused_variables)]
pub trait Service<C: CoreApi> {
    /// The rule for the records of kind `kind` that this service owns, as far as the bodies of one module's records of that kind decide it: `bodies` are the bodies (after the
    /// 8-byte header) in file order. Calls `report` for each problem, none when the set is good. It reads nothing but its arguments, so a host tool runs it on a built module
    /// (`patchkit records --check`), and the service's own `check` runs the same code at load (so a module that passes here is not refused at load for these rules). What
    /// needs the device, the module's image, the section's other kinds or the registry state is not covered; it stays in `check`, and the service's own doc names it.
    /// A kind the service does not own: accept. A service without rules writes `dkrt_abi::accepts_records!();`. Required: a service says what it checks, even if that is
    /// nothing. The callback is `&mut dyn FnMut(Problem)` so the method has one shape for every service, also one built in another object.
    ///
    /// ```ignore
    /// fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    ///     if kind == KIND_X && bodies.len() > 8 {
    ///         report(Problem::set(kind, E_FORMAT, "more than 8 records"));
    ///     }
    /// }
    /// ```
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem));

    /// Whether module record `owner - 1` may be reloaded now, as far as this service knows: `OK`, or the status that refuses (`E_RELOAD_ROUTED`: an effect of the
    /// module is on a route). The reload job asks every service after the generic refusals (`reload::refusal`). A service with nothing to wait for writes
    /// `dkrt_abi::accepts_teardown!();`.
    unsafe fn teardown_refusal(core: &C, owner: u8) -> i32;
    /// A reload of a module ended, with the new version loaded or not: give back what this service kept for the module during the reload (a placeholder row) that
    /// the new version did not take over. Called on every service that is loaded, once per reload.
    unsafe fn live_done(core: &C) {}
    /// The service's declarations (`Declare`): called once at the runtime's init; the runtime compares them with the service's table.
    unsafe fn declare(d: &mut Declare) {}
    /// One step of the validation (`check_step`) for this service's records. Writes nothing.
    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        OK
    }
    /// Write this service's rows that the old body wrote before its per-record pass (parameters, actions, pages, machines).
    unsafe fn apply(core: &C, c: &ApplyCtx) -> i32 {
        OK
    }
    /// Write one record of this service's kinds, in file order after every `apply` (native pages and effect pages interleave as in the
    /// file). A kind this service does not own: OK.
    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        OK
    }
    /// Give back everything module record `owner - 1` filled.
    unsafe fn remove(core: &C, owner: u8) {}

    /// `(limit, used)` of capacity kind `kind` (`proto::CAPK_*`) if this service counts it; a kind two services count is the sum of their counts.
    unsafe fn cap(core: &C, kind: u8) -> Option<(u32, u32)> {
        None
    }
    /// The module record behind a fault site and the id the DSP reported there (`fault::SITE_*`), as the DSP reported it: a voice site gives the machine type,
    /// an effect site the byte offset of the row in the table of the effect chain. The service turns it into its own row.
    unsafe fn attribute(core: &C, site: u32, id: u32) -> Option<usize> {
        None
    }
    /// The polled DSP diagnostics block (`DIAG_WORDS` words of `dl_diag`), once for each poll that returned a full block. A service reads the words it owns and logs
    /// its events (`core.log`); what it has seen already is its own state. The core keeps the heartbeat watch, the raw log of the words and the hang record.
    unsafe fn diag(core: &C, words: &[u32; DIAG_WORDS]) {}
    /// The service's tables against the module records: bit `i` of `used` is set when module record `i` is used. 0 when the tables agree; else a number (not 0) and the
    /// first mismatch in `out`. The core runs it with its own record checks (invariant 3) and logs a `TABLE_BAD`; the contract suite runs it after every step.
    unsafe fn table_check(core: &C, used: u32, out: &mut TableFault) -> u32 {
        0
    }
    /// Every row of module `rec` is in the DSP.
    unsafe fn on_loaded(core: &C, rec: usize) {}
    /// The service's work of one UI-loop pass in `phase` (`tick_phase`).
    unsafe fn tick(core: &C, phase: u32) {}

    /// The `init` of module record `rec` is about to run: forget what an earlier module collected. Called on every service that is loaded.
    unsafe fn module_begin(core: &C, rec: usize) {}
    /// The commit of module `rec`, after its `init` (`commit_phase`: a `PREPARE` pass, then a `WRITE` pass). A service checks what its builder calls collected, writes its
    /// records into `c.sink` and returns `OK` or the refusal; the first refusal stops the commit and the loader gives every service its `remove`.
    unsafe fn module_commit(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
        OK
    }
    /// Boot assigns its accepted selection once. A live candidate receives a preview followed by
    /// restore or commit. Preview may calculate provisional numbers but cannot persist or publish them.
    /// Restore gives back the exact active assignments. Commit keeps the accepted assignments.
    unsafe fn settle(core: &C, s: &SettleCtx) {}

    /// DSP section: the entries of the tables this service owns (called in list order, after the header is validated), and the header's counts of them. The service
    /// adds the bytes of the state blocks its entries need behind the image (`job.state_bytes`, each block rounded to 8): the core adds them to the range it reserves.
    unsafe fn dsp_check_sections(core: &C, sec: &DspSec, job: &mut Job) -> i32 {
        OK
    }
    /// DSP section: free rows for this service's entries.
    unsafe fn dsp_check_rows(core: &C, sec: &DspSec) -> i32 {
        OK
    }
    /// DSP section: the machine types of the voices (after every service's rows were checked).
    unsafe fn dsp_check_types(core: &C, sec: &DspSec) -> i32 {
        OK
    }
    /// Refuse a reservation before anything is reserved (`OK` or an `E_*`).
    unsafe fn dsp_admit(core: &C, job: &Job) -> i32 {
        OK
    }
    /// Claim this service's rows for module `rec`.
    unsafe fn dsp_claim(core: &C, c: &mut Claim) {}
    /// Give back what `dsp_claim` took (the rows are cleared).
    unsafe fn dsp_release(core: &C, rec: usize) {}
    /// Free the rows of module `rec` this service published in the DSP; false when a write failed.
    unsafe fn dsp_clear(core: &C, rec: usize) -> bool {
        true
    }
    /// The next word to publish for module `rec`: `want` counts the steps since the first row; a row of this service takes its steps off
    /// `want`, and the row that holds the step returns its write. The word is read from the row only when `write` is set (a step in
    /// flight needs to know that a row exists, not what it writes).
    unsafe fn dsp_row_step(core: &C, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        None
    }
}

/// The version of `SettingsOverlay`.
pub const SETTINGS_OVERLAY_VERSION: u32 = 1;

/// A layer over the settings of the modules that a service gives the runtime: the entry points the runtime's settings calls forward to. The service file sets one table with
/// `dkc_settings_overlay_set` in `init` and the runtime drops the pointer when the file's record goes away, so the runtime holds at most one table and the settings calls of a
/// module find the layer or do nothing. All three are called in the UI task. `id` is the NUL-free module id (the runtime took it from the module's record, never from the
/// caller), `key` the module's bare key; the layer stores `<id>/<key>` and applies the key and value rules of the settings record. The results are those of
/// `dk_settings_get`, `dk_settings_put` and `dk_settings_remove`. `put` and `remove` answer `E_SET_OFF` when the layer has nothing to hold now (the provider has no data
/// to layer): the runtime reads that as no refusal and the record alone takes the change. Every other negative answer (`E_SET_FULL`, `E_SET_KEY`, `E_SET_VALUE`, any other
/// code) is the module's answer and the record stays as it was. The runtime has checked before the call that the record can take the change.
#[repr(C)]
pub struct SettingsOverlay {
    pub version: u32,
    pub get: unsafe extern "C" fn(
        id: *const u8,
        id_len: u32,
        key: *const u8,
        key_len: u32,
        out: *mut u8,
        cap: u32,
    ) -> i32,
    pub put: unsafe extern "C" fn(
        id: *const u8,
        id_len: u32,
        key: *const u8,
        key_len: u32,
        value: *const u8,
        len: u32,
    ) -> i32,
    pub remove:
        unsafe extern "C" fn(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32,
}

// Read-only data of a service file.
unsafe impl Sync for SettingsOverlay {}

/// One service in the table, as the core sees it: its name, the record kinds it owns and its methods as `extern "C"` thunks. The row is the
/// same type for every service and every core: `ServiceRow::of::<S, C>` builds it from `impl Service<C> for S` with generic thunks, so a row
/// built in another object (a service loaded from the card) has the same layout. A method the service does not implement points at the
/// default (`OK`, nothing). The first argument of every thunk is the core (`*const c_void`, a `&C` of the `C` the row was built for).
/// Results that are `Option` and values that are arrays travel C-shaped: out pointers with a `bool` and pointers to the words.
#[repr(C)]
pub struct ServiceRow {
    /// The name of `[[service]]` in sdk/sdk.toml (the owner a kind's declaration names): `name_len` bytes of UTF-8.
    pub name: *const u8,
    pub name_len: u32,
    /// The record kinds of the `register` section the service owns: `nkinds` pairs of (kind, whole record length with its header), from
    /// `[records.kinds]`.
    pub kinds: *const (u32, u32),
    pub nkinds: u32,
    declare: unsafe extern "C" fn(*const c_void, *mut Declare),
    check: unsafe extern "C" fn(*const c_void, u32, *const CheckCtx) -> i32,
    apply: unsafe extern "C" fn(*const c_void, *const ApplyCtx) -> i32,
    apply_record: unsafe extern "C" fn(*const c_void, u32, *const u8, *const ApplyCtx) -> i32,
    remove: unsafe extern "C" fn(*const c_void, u8),
    cap: unsafe extern "C" fn(*const c_void, u8, *mut u32) -> bool,
    attribute: unsafe extern "C" fn(*const c_void, u32, u32, *mut usize) -> bool,
    diag: unsafe extern "C" fn(*const c_void, *const u32),
    table_check: unsafe extern "C" fn(*const c_void, u32, *mut TableFault) -> u32,
    on_loaded: unsafe extern "C" fn(*const c_void, usize),
    settle: unsafe extern "C" fn(*const c_void, *const SettleCtx),
    teardown_refusal: unsafe extern "C" fn(*const c_void, u8) -> i32,
    live_done: unsafe extern "C" fn(*const c_void),
    module_begin: unsafe extern "C" fn(*const c_void, usize),
    module_commit: unsafe extern "C" fn(*const c_void, usize, *const ModuleCommitCtx) -> i32,
    tick: unsafe extern "C" fn(*const c_void, u32),
    dsp_check_sections: unsafe extern "C" fn(*const c_void, *const DspSec, *mut Job) -> i32,
    dsp_check_rows: unsafe extern "C" fn(*const c_void, *const DspSec) -> i32,
    dsp_check_types: unsafe extern "C" fn(*const c_void, *const DspSec) -> i32,
    dsp_admit: unsafe extern "C" fn(*const c_void, *const Job) -> i32,
    dsp_claim: unsafe extern "C" fn(*const c_void, *mut Claim),
    dsp_release: unsafe extern "C" fn(*const c_void, usize),
    dsp_clear: unsafe extern "C" fn(*const c_void, usize) -> bool,
    dsp_row_step: unsafe extern "C" fn(*const c_void, usize, *mut u32, bool, *mut RowWrite) -> bool,
}

// A row is built from constants and never written.
unsafe impl Sync for ServiceRow {}

/// The thunks of `ServiceRow::of`: each casts the core back to `&C` and calls the method of `S`. A method that returns an `Option` writes the
/// value to its out pointer and returns whether there was one.
mod thunk {
    use super::*;

    pub unsafe extern "C" fn declare<C: CoreApi, S: Service<C>>(_: *const c_void, d: *mut Declare) {
        S::declare(&mut *d)
    }
    pub unsafe extern "C" fn check<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        step: u32,
        c: *const CheckCtx,
    ) -> i32 {
        S::check(&*(core as *const C), step, &*c)
    }
    pub unsafe extern "C" fn apply<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        c: *const ApplyCtx,
    ) -> i32 {
        S::apply(&*(core as *const C), &*c)
    }
    pub unsafe extern "C" fn apply_record<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        kind: u32,
        body: *const u8,
        c: *const ApplyCtx,
    ) -> i32 {
        S::apply_record(&*(core as *const C), kind, body, &*c)
    }
    pub unsafe extern "C" fn remove<C: CoreApi, S: Service<C>>(core: *const c_void, owner: u8) {
        S::remove(&*(core as *const C), owner)
    }
    /// `out` points at two words: limit, used.
    pub unsafe extern "C" fn cap<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        kind: u8,
        out: *mut u32,
    ) -> bool {
        match S::cap(&*(core as *const C), kind) {
            Some((l, u)) => {
                *out = l;
                *out.add(1) = u;
                true
            }
            None => false,
        }
    }
    pub unsafe extern "C" fn attribute<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        site: u32,
        id: u32,
        out: *mut usize,
    ) -> bool {
        match S::attribute(&*(core as *const C), site, id) {
            Some(m) => {
                *out = m;
                true
            }
            None => false,
        }
    }
    /// `words` points at `DIAG_WORDS` words.
    pub unsafe extern "C" fn diag<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        words: *const u32,
    ) {
        S::diag(&*(core as *const C), &*(words as *const [u32; DIAG_WORDS]))
    }
    pub unsafe extern "C" fn table_check<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        used: u32,
        out: *mut TableFault,
    ) -> u32 {
        S::table_check(&*(core as *const C), used, &mut *out)
    }
    pub unsafe extern "C" fn on_loaded<C: CoreApi, S: Service<C>>(core: *const c_void, rec: usize) {
        S::on_loaded(&*(core as *const C), rec)
    }
    pub unsafe extern "C" fn settle<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        s: *const SettleCtx,
    ) {
        S::settle(&*(core as *const C), &*s)
    }
    pub unsafe extern "C" fn teardown_refusal<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        owner: u8,
    ) -> i32 {
        S::teardown_refusal(&*(core as *const C), owner)
    }
    pub unsafe extern "C" fn live_done<C: CoreApi, S: Service<C>>(core: *const c_void) {
        S::live_done(&*(core as *const C))
    }
    pub unsafe extern "C" fn module_begin<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        rec: usize,
    ) {
        S::module_begin(&*(core as *const C), rec)
    }
    pub unsafe extern "C" fn module_commit<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        rec: usize,
        c: *const ModuleCommitCtx,
    ) -> i32 {
        S::module_commit(&*(core as *const C), rec, &*c)
    }
    pub unsafe extern "C" fn tick<C: CoreApi, S: Service<C>>(core: *const c_void, phase: u32) {
        S::tick(&*(core as *const C), phase)
    }
    pub unsafe extern "C" fn dsp_check_sections<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        sec: *const DspSec,
        job: *mut Job,
    ) -> i32 {
        S::dsp_check_sections(&*(core as *const C), &*sec, &mut *job)
    }
    pub unsafe extern "C" fn dsp_check_rows<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        sec: *const DspSec,
    ) -> i32 {
        S::dsp_check_rows(&*(core as *const C), &*sec)
    }
    pub unsafe extern "C" fn dsp_check_types<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        sec: *const DspSec,
    ) -> i32 {
        S::dsp_check_types(&*(core as *const C), &*sec)
    }
    pub unsafe extern "C" fn dsp_admit<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        job: *const Job,
    ) -> i32 {
        S::dsp_admit(&*(core as *const C), &*job)
    }
    pub unsafe extern "C" fn dsp_claim<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        c: *mut Claim,
    ) {
        S::dsp_claim(&*(core as *const C), &mut *c)
    }
    pub unsafe extern "C" fn dsp_release<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        rec: usize,
    ) {
        S::dsp_release(&*(core as *const C), rec)
    }
    pub unsafe extern "C" fn dsp_clear<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        rec: usize,
    ) -> bool {
        S::dsp_clear(&*(core as *const C), rec)
    }
    pub unsafe extern "C" fn dsp_row_step<C: CoreApi, S: Service<C>>(
        core: *const c_void,
        rec: usize,
        want: *mut u32,
        write: bool,
        out: *mut RowWrite,
    ) -> bool {
        match S::dsp_row_step(&*(core as *const C), rec, &mut *want, write) {
            Some(w) => {
                *out = w;
                true
            }
            None => false,
        }
    }
}

impl ServiceRow {
    /// The row of service `S` over core `C` that owns `kinds`.
    pub const fn of<S: Service<C>, C: CoreApi + 'static>(
        name: &'static str,
        kinds: &'static [(u32, u32)],
    ) -> ServiceRow {
        ServiceRow {
            name: name.as_ptr(),
            name_len: name.len() as u32,
            kinds: kinds.as_ptr(),
            nkinds: kinds.len() as u32,
            declare: thunk::declare::<C, S>,
            check: thunk::check::<C, S>,
            apply: thunk::apply::<C, S>,
            apply_record: thunk::apply_record::<C, S>,
            remove: thunk::remove::<C, S>,
            cap: thunk::cap::<C, S>,
            attribute: thunk::attribute::<C, S>,
            diag: thunk::diag::<C, S>,
            table_check: thunk::table_check::<C, S>,
            on_loaded: thunk::on_loaded::<C, S>,
            settle: thunk::settle::<C, S>,
            teardown_refusal: thunk::teardown_refusal::<C, S>,
            live_done: thunk::live_done::<C, S>,
            module_begin: thunk::module_begin::<C, S>,
            module_commit: thunk::module_commit::<C, S>,
            tick: thunk::tick::<C, S>,
            dsp_check_sections: thunk::dsp_check_sections::<C, S>,
            dsp_check_rows: thunk::dsp_check_rows::<C, S>,
            dsp_check_types: thunk::dsp_check_types::<C, S>,
            dsp_admit: thunk::dsp_admit::<C, S>,
            dsp_claim: thunk::dsp_claim::<C, S>,
            dsp_release: thunk::dsp_release::<C, S>,
            dsp_clear: thunk::dsp_clear::<C, S>,
            dsp_row_step: thunk::dsp_row_step::<C, S>,
        }
    }
    /// The service's name.
    pub fn name(&self) -> &'static str {
        unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                self.name,
                self.name_len as usize,
            ))
        }
    }
    /// The record kinds the service owns: (kind, record length).
    pub fn kinds(&self) -> &'static [(u32, u32)] {
        unsafe { core::slice::from_raw_parts(self.kinds, self.nkinds as usize) }
    }
}

/// The row pointers of a table, in a static (a raw pointer is not `Sync`; the rows are read-only).
#[repr(transparent)]
pub struct RowPtrs<const N: usize>(pub [*const ServiceRow; N]);

unsafe impl<const N: usize> Sync for RowPtrs<N> {}

impl<const N: usize> RowPtrs<N> {
    /// The pointers to `rows`, in order.
    pub const fn of(rows: &'static [ServiceRow; N]) -> RowPtrs<N> {
        let mut a = [core::ptr::null::<ServiceRow>(); N];
        let mut i = 0;
        while i < N {
            a[i] = &rows[i] as *const ServiceRow;
            i += 1;
        }
        RowPtrs(a)
    }
}

/// The pointers to `rows` followed by null pointers up to `M` entries (`M >= N`): the table of a core that adds the rows of loaded service files after its own.
pub const fn rows_with_tail<const N: usize, const M: usize>(
    rows: &'static [ServiceRow; N],
) -> [*const ServiceRow; M] {
    let mut a = [core::ptr::null::<ServiceRow>(); M];
    let mut i = 0;
    while i < N {
        a[i] = &rows[i] as *const ServiceRow;
        i += 1;
    }
    a
}

/// The table of services, in the order the core calls them: the walks below run the rows in table order (removal runs the other way). It is the
/// same type for every core; `core` is the `&C` that each row's thunks cast back.
#[derive(Clone, Copy)]
pub struct ServiceTable {
    pub rows: &'static [*const ServiceRow],
    /// Bit `i` set: row `i` FAILED its declare check (`check_declarations`); its kinds are unknown to `owner` and `len`.
    pub failed: u32,
    /// The core every call passes to the service (`&REAL_CORE` in the runtime's generated table).
    pub core: *const c_void,
}

// A table is read-only data (`rows` and `core` point at statics).
unsafe impl Sync for ServiceTable {}

impl ServiceTable {
    /// A table over `rows` that passes `core` to every method.
    #[inline]
    pub const fn new<C: CoreApi + 'static>(
        rows: &'static [*const ServiceRow],
        core: &'static C,
    ) -> ServiceTable {
        ServiceTable {
            rows,
            failed: 0,
            core: core as *const C as *const c_void,
        }
    }
    #[inline(always)]
    fn row(&self, i: usize) -> &'static ServiceRow {
        unsafe { &*self.rows[i] }
    }
    /// Declare-check-commit of the in-crate services: each row's `declare` against its table (the generated static `kinds`). Returns the mask of the
    /// rows whose declarations differ (bit `i`: row `i`); the caller keeps it in the table it hands out (`failed`).
    #[inline]
    pub unsafe fn check_declarations(&self) -> u32 {
        let mut failed = 0u32;
        let mut i = 0;
        while i < self.rows.len() && i < 32 {
            let mut d = Declare::new();
            (self.row(i).declare)(self.core, &mut d);
            if !d.matches_kinds(self.row(i).kinds()) {
                failed |= 1 << i;
            }
            i += 1;
        }
        failed
    }
    /// The row that owns record kind `kind`.
    #[inline]
    pub fn owner(&self, kind: u32) -> Option<&'static ServiceRow> {
        let mut i = 0;
        while i < self.rows.len() {
            if i < 32 && self.failed & (1 << i) != 0 {
                i += 1;
                continue;
            }
            let kinds = self.row(i).kinds();
            let mut k = 0;
            while k < kinds.len() {
                if kinds[k].0 == kind {
                    return Some(self.row(i));
                }
                k += 1;
            }
            i += 1;
        }
        None
    }
    /// The length of a record of `kind` (the whole record, header included); `None` when no service owns the kind.
    #[inline]
    pub fn len(&self, kind: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if i < 32 && self.failed & (1 << i) != 0 {
                i += 1;
                continue;
            }
            let kinds = self.row(i).kinds();
            let mut k = 0;
            while k < kinds.len() {
                if kinds[k].0 == kind {
                    return Some(kinds[k].1);
                }
                k += 1;
            }
            i += 1;
        }
        None
    }
    #[inline]
    pub unsafe fn check(&self, step: u32, c: &CheckCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).check)(self.core, step, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn apply(&self, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).apply)(self.core, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn apply_record(&self, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).apply_record)(self.core, kind, body, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// Last service first.
    #[inline]
    pub unsafe fn remove(&self, owner: u8) {
        let mut i = self.rows.len();
        while i > 0 {
            i -= 1;
            (self.row(i).remove)(self.core, owner);
        }
    }
    /// `(limit, used)` of capacity kind `kind`: the sum of the counts of the services that count it; `None` when none does.
    #[inline]
    pub unsafe fn cap(&self, kind: u8) -> Option<(u32, u32)> {
        let mut r: Option<(u32, u32)> = None;
        let mut i = 0;
        while i < self.rows.len() {
            let mut lu = [0u32; 2];
            if (self.row(i).cap)(self.core, kind, lu.as_mut_ptr()) {
                let (l, u) = (lu[0], lu[1]);
                r = Some(match r {
                    Some((a, b)) => (a + l, b + u),
                    None => (l, u),
                });
            }
            i += 1;
        }
        r
    }
    #[inline]
    pub unsafe fn attribute(&self, site: u32, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            let mut m = 0usize;
            if (self.row(i).attribute)(self.core, site, id, &mut m) {
                return Some(m);
            }
            i += 1;
        }
        None
    }
    /// `Service::diag` on every row, in table order.
    #[inline]
    pub unsafe fn diag(&self, words: &[u32; DIAG_WORDS]) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).diag)(self.core, words.as_ptr());
            i += 1;
        }
    }
    /// `Service::table_check` over the rows in table order: the first number that is not 0 (the mismatch is in `out`), else 0.
    #[inline]
    pub unsafe fn table_check(&self, used: u32, out: &mut TableFault) -> u32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).table_check)(self.core, used, out);
            if r != 0 {
                return r;
            }
            i += 1;
        }
        0
    }
    #[inline]
    pub unsafe fn on_loaded(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).on_loaded)(self.core, rec);
            i += 1;
        }
    }
    /// `Service::settle` on every row, in table order.
    #[inline]
    pub unsafe fn settle(&self, s: &SettleCtx) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).settle)(self.core, s);
            i += 1;
        }
    }
    /// `Service::teardown_refusal` on every row, in table order: the first status that is not `OK`, else `OK`.
    #[inline]
    pub unsafe fn teardown_refusal(&self, owner: u8) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).teardown_refusal)(self.core, owner);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Service::live_done` on every row, in table order.
    #[inline]
    pub unsafe fn live_done(&self) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).live_done)(self.core);
            i += 1;
        }
    }
    /// `Service::module_begin` on every row, in table order.
    #[inline]
    pub unsafe fn module_begin(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).module_begin)(self.core, rec);
            i += 1;
        }
    }
    /// `Service::module_commit` over the rows in table order; the first refusal stops the walk and is the result.
    #[inline]
    pub unsafe fn module_commit(&self, rec: usize, c: &ModuleCommitCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).module_commit)(self.core, rec, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn tick(&self, phase: u32) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).tick)(self.core, phase);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_check_sections(&self, sec: &DspSec, job: &mut Job) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).dsp_check_sections)(self.core, sec, job);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_check_rows(&self, sec: &DspSec) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).dsp_check_rows)(self.core, sec);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_check_types(&self, sec: &DspSec) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).dsp_check_types)(self.core, sec);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_admit(&self, job: &Job) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = (self.row(i).dsp_admit)(self.core, job);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_claim(&self, c: &mut Claim) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).dsp_claim)(self.core, c);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_release(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            (self.row(i).dsp_release)(self.core, rec);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_clear(&self, rec: usize) -> bool {
        let mut ok = true;
        let mut i = 0;
        while i < self.rows.len() {
            ok = (self.row(i).dsp_clear)(self.core, rec) && ok;
            i += 1;
        }
        ok
    }
    #[inline]
    pub unsafe fn dsp_row_step(&self, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        let mut i = 0;
        while i < self.rows.len() {
            let mut w = RowWrite {
                win: 0,
                word: 0,
                value: 0,
                published: None,
                last: false,
                has_undo: false,
                undo: 0,
            };
            if (self.row(i).dsp_row_step)(self.core, rec, want, write, &mut w) {
                return Some(w);
            }
            i += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_is_an_item_of_its_own_that_the_header_table_cannot_hold() {
        let row = 0x1234usize as *const ServiceRow;
        let mut d = Declare::new();
        d.kind(90, 24);
        assert_eq!(d.row_ptr(), None);
        d.row(row);
        assert_eq!(d.n, 2);
        assert_eq!(d.row_ptr(), Some(row));
        // the table has the kind alone: the declarations with a row are the same as it
        let mut table = Declare::new();
        table.kind(90, 24);
        assert!(d.same_as(&table));
        // a table that has more, or another kind, is not the same
        let mut more = table;
        more.slot(1, 0, 0);
        assert!(!d.same_as(&more));
        let mut other = Declare::new();
        other.kind(91, 24);
        assert!(!d.same_as(&other));
        // and the walk of a static service's kinds does not take a row for a kind
        assert!(!d.matches_kinds(&[(90, 24)]));
    }

    #[test]
    fn the_fingerprint_is_not_zero_and_depends_on_the_files_hash() {
        assert_ne!(SVC_ABI, 0);
        assert_ne!(crate::gen_svc_abi::FILES_HASH, 0);
    }
}
