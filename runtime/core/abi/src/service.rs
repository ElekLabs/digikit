//! Interface B of docs/ARCHITECTURE.md: the contexts and records the core hands a service, and the ABI fingerprint of the boundary.
//!
//! A service is a provider (`provider`): a `ProviderRow` of part tables over an owner object. The core keeps one table of rows (`ServiceTable` over the
//! `ProviderHandle`s of the loaded service files, in callback order) and walks it; this file holds what those walks pass: `CheckCtx`, `ApplyCtx`,
//! `Claim`, `RowWrite`, `ModuleCommitCtx` with its `RecordSink`, `SettleCtx`, the `Declare` a file makes in its `init`, and `SVC_ABI`.
use crate::dsp::{DspSec, Job};
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

/// The first mismatch `Describe::table_check` found: the module record index, and two numbers that say which row and what it held (the service's own). The core
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

/// What the loader gives `settle`: the keys of each module of the boot's selection that a service numbers (the keys table of its header; `nkeys[k]` of the 8 words of
/// `keys[k]`), in selection order. A module whose file could not be read has none. What the keys name is the service's business.
#[repr(C)]
pub struct SettleCtx {
    /// The modules of the selection.
    pub count: usize,
    pub nkeys: [u8; dkrt_sdk::SEL_MAX],
    pub keys: [[u32; 8]; dkrt_sdk::SEL_MAX],
}

impl SettleCtx {
    pub const fn new() -> SettleCtx {
        SettleCtx {
            count: 0,
            nkeys: [0; dkrt_sdk::SEL_MAX],
            keys: [[0; 8]; dkrt_sdk::SEL_MAX],
        }
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
/// (the header table of a module) and a difference means the service FAILED.
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
    /// The `ProviderRow` a service file declares (`dk_provider_declare_row`): the row's address in `a` and `b` (low and high word) and the instance's address in `c` and
    /// the next item (`PROVIDER_INSTANCE`, the same form). The header table has no such entry.
    pub const PROVIDER_ROW: u16 = 10;
    pub const PROVIDER_INSTANCE: u16 = 11;
    /// A patch the service takes (`dk_service_declare_patch`): the name hash of the patch site in `a`, the value in `b` (its low `c` bytes, big-endian) and the length `c` (1, 2
    /// or 4). The header table has a `HOOKS` item with the entry `HOOK_ENTRY_PATCH` for it (the value is a run-time fact and is not in the file); `Declare::as_table` maps it so.
    pub const PATCH: u16 = 12;

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
    /// A patch of `len` bytes (1, 2 or 4) with `value` at the patch site `site` (the name hash).
    pub fn patch(&mut self, site: u32, value: u32, len: u32) {
        self.add(Declare::PATCH, site, value, len);
    }
    /// The provider file's row and its instance.
    pub fn provider_row(&mut self, row: *const crate::provider::ProviderRow, instance: *mut u8) {
        let at = row as usize as u64;
        self.add(Declare::PROVIDER_ROW, at as u32, (at >> 32) as u32, 0);
        let at = instance as usize as u64;
        self.add(Declare::PROVIDER_INSTANCE, at as u32, (at >> 32) as u32, 0);
    }
    /// The row and instance of the `PROVIDER_ROW` and `PROVIDER_INSTANCE` items, if a provider row was declared.
    pub fn provider_row_ptr(&self) -> Option<(*const crate::provider::ProviderRow, *mut u8)> {
        let (mut row, mut instance) = (None, None);
        let mut i = 0;
        while i < self.n {
            let DeclItem { tag, a, b, .. } = self.items[i];
            let at = (((b as u64) << 32 | a as u64) as usize) as *mut u8;
            if tag == Declare::PROVIDER_ROW {
                row = Some(at as *const crate::provider::ProviderRow);
            } else if tag == Declare::PROVIDER_INSTANCE {
                instance = Some(at);
            }
            i += 1;
        }
        Some((row?, instance?))
    }
    /// Whether `tag` is one of the items that carry a file's row (`PROVIDER_ROW`, `PROVIDER_INSTANCE`): the header table has none.
    const fn is_row_item(tag: u16) -> bool {
        tag == Declare::PROVIDER_ROW || tag == Declare::PROVIDER_INSTANCE
    }
    /// Whether `other` holds the same items (in any order): what a module declared in `init` against what its header table says.
    ///
    /// The row items of `self` (a service file's own row and instance) have no entry in a header table and are left out.
    pub fn same_as(&self, other: &Declare) -> bool {
        let mut rows = 0;
        let mut r = 0;
        while r < self.n {
            if Declare::is_row_item(self.items[r].tag) {
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
            if !Declare::is_row_item(self.items[i].tag) && !has(other, self.items[i]) {
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

/// One refusal of a set of records, from `Records::validate`. The same value serves the load path (`code` is what the loader returns) and a build-time
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

/// How a service file names the firmware. 2: through the port file (docs/plans/port-file.md): `dk_fw_`, `dk_exit_`, `dk_tail_`, `dk_stock_` and `dk_ret_` imports that the
/// loader resolves from it, and raw `HOOKS` entries whose sites it lists; the file has a hard `NEED` of `port` and no `BIND` or `FW` item. 1 was the file bound to one
/// firmware by the port file. The constant is part of the boundary files, so a change of it changes `SVC_ABI` and the runtime refuses the older files (`E_SVC_ABI`).
pub const FIRMWARE_REFERENCES: u32 = 2;

/// The device mask in the flags of a `SLOTS` item of a service file (`dkneeds::keys::slot_for_device`): bits 16 to 23, bit `id - 1` for the device with runtime id `id`, none for
/// every device. The loader skips an item for another device, so one file serves every device. The constant is part of the boundary files: a runtime without the mask would
/// refuse such an item, so the change of this layout changes `SVC_ABI` and the install refuses the mismatch before boot.
pub const SLOT_DEVICES_MASK: u32 = dkneeds::keys::SLOT_DEVICES_MASK;

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

pub use crate::service_table::ServiceTable;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_is_not_zero_and_depends_on_the_files_hash() {
        assert_ne!(SVC_ABI, 0);
        assert_ne!(crate::gen_svc_abi::FILES_HASH, 0);
    }
}
