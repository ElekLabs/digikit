//! The service contract suite (docs/plans/extension-model.md 3.5): one generic function, `contract::<S>(&Fixture)`, that every service crate runs in its own
//! `tests/contract.rs`. It proves the reload property of a service on the host: a module that is loaded, unloaded and loaded again leaves the service in the same
//! state, and the loads and unloads of one module do not touch another module's rows.
//!
//! `Lifecycle` runs a module the way the loader does (`loader/mod.rs`, `complete`, `commit_services` and `unload_rec_inner`) over a `FakeCore`: the DSP reservation
//! (`dsp_admit`, `dsp_claim`), `module_begin`, the module's `init` (its builder calls), `module_commit` (the `PREPARE` and `WRITE` passes), the registry walker's
//! check, the `DECLARE` pass, the walker's apply (`apply`, then `apply_record` for each record), and the publication of the DSP rows. A refusal at any step gives the
//! services their `remove` and the DSP clear and release, as the loader's `fail` does; an unload does `dsp_clear`, `remove` and `dsp_release`. The registry walker's layout
//! check is `dkrt_abi::regcheck::check_layout`, the function the runtime's walker calls; the suite calls it with the kinds of the one service under test.
//!
//! A module that is loaded and unloaded must leave nothing: case 7 compares the whole state after "load `A`, unload `A`" with the state after "load `a2`, unload `a2`"
//! (`a2` has the keys of `A` and other content in every table the service fills), and compares the state after the unload with the state before the load: a memory
//! word, DSP state byte or setting that differs from the start is a leftover unless the fixture's `kept` or `kept_dsp` names it with the value it may hold.
//!
//! The call lists that stand for the shared fixtures (`tests/fixtures/modules`) are in each service's `tests/contract.rs`: they name the service's own `build::*`, and this
//! crate cannot depend on a service.
use crate::FakeCore;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{
    DspState, Job, FX_DESC, FX_PARAMS, FX_PROCESS, FX_REC, FX_STATE, L2_BYTE_BASE,
    VOICE_CLEAR_BYTES, VOICE_ENTRY, VOICE_LATCH, VOICE_NOTE_ON, VOICE_STATE_BYTES, VOICE_TYPE,
    VOICE_V3, VOICE_V3_RANGES, VOICE_VOICES,
};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::regcheck::check_layout;
use dkrt_abi::registry::each;
use dkrt_abi::service::{
    check_step, commit_phase, ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, RecordSink,
    Service, SettleCtx, TableFault,
};
use dkrt_abi::MAX_MODULES;
use dkrt_sdk::*;
use std::collections::{BTreeMap, BTreeSet};
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

/// The DSP section a module brings: how many voice and effect rows its records claim (the fields of the entries follow `salt`), and whether it has a track-table record
/// (version 5; the module's local id).
#[derive(Clone, Copy, Default)]
pub struct Dsp {
    pub voices: u32,
    pub fxs: u32,
    pub bank: Option<u32>,
    /// A number that sets the fields of the module's voice and effect entries to other values (0: the fields stay 0). Two modules with the same keys and other
    /// salts claim the same rows with other contents, so a row word that a release leaves behind shows.
    pub salt: u32,
}

impl Dsp {
    fn any(&self) -> bool {
        self.voices + self.fxs > 0 || self.bank.is_some()
    }
}

/// One module of a fixture: its place in the selection, its header's keys, its `init` and its DSP rows (a voice for each machine key of the keys table, an effect
/// record for each `fxs`).
pub struct Module {
    /// The module's index in the boot's selection (`CoreApi::boot_index` while it loads; `settle` gets the keys of every module of the fixture in this order).
    pub sel: u32,
    pub keys: fn() -> KeysCopy,
    /// The keys of other record kinds the header table lists, (kind, keys): what `CoreApi::keys_of` gives a service that numbers or checks them.
    pub groups: &'static [(u32, &'static [u32])],
    pub init: fn(&mut Init),
    pub dsp: Dsp,
}

/// The row tables a service keeps in statics of its own (`DKRT_*`, not in the loader's `DspState`): a view of their bytes for the snapshots, the reset to the start state
/// that a fresh core has, the bytes a release leaves other than at the start, and the views of the cases that compare a module loaded beside a parked row: the bytes
/// without the placeholder rows and with the rows in the order of their types. A service without row tables has `Rows::NONE`.
#[derive(Clone, Copy)]
pub struct Rows {
    /// The bytes of the service's row tables now.
    pub bytes: fn() -> Vec<u8>,
    /// Back to the start state (zero): a core starts with the tables as the static was linked.
    pub reset: fn(),
    /// `(byte offset in `bytes`, value)` pairs a release leaves other than at the start, on purpose. Each entry has a reason in the fixture.
    pub kept: fn(&FakeCore, usize, u8) -> bool,
    /// The bytes with each MISSING placeholder row as a free row (a comparison of services that park the rows of absent modules leaves out the row bytes that are not the
    /// module's own). The identity for a service that parks no row.
    pub without_placeholders: fn(&[u8]) -> Vec<u8>,
    /// The bytes (already `without_placeholders`) with the rows in the order of their types, free rows last, and the row masks following: a module loaded beside a parked row
    /// has its row at another index than the same module loaded alone, and its words are compared by type. The identity for a service that parks no row.
    pub by_type: fn(&[u8]) -> Vec<u8>,
    /// The number of rows that are placeholders in the bytes.
    pub placeholder_rows: fn(&[u8]) -> usize,
}

impl Rows {
    /// A service with no row tables.
    pub const NONE: Rows = Rows {
        bytes: Vec::new,
        reset: || {},
        kept: |_, _, _| false,
        without_placeholders: |b| b.to_vec(),
        by_type: |b| b.to_vec(),
        placeholder_rows: |_| 0,
    };
}

/// What a service gives the suite: the profile, the kinds it owns (`[records.kinds]`), the image tables its core has, its modules and a view of its own state.
pub struct Fixture {
    pub name: &'static str,
    /// The device id of the profile (`dkrt_abi::profiles::ALL`).
    pub device: u32,
    /// The record kinds of `[records.kinds]` of the service: (kind, whole record length).
    pub kinds: &'static [(u32, u32)],
    /// The image's tables of the profile (`info_tables`, `host`), set before the first module loads.
    pub image: fn(&mut FakeCore),
    /// What the service keeps that the firmware memory does not show (its tables of owners and ids), as words: its own query functions.
    pub state: fn(&FakeCore) -> Vec<u32>,
    /// Module A and module B: different keys, the same shape.
    pub a: Module,
    pub b: Module,
    /// A module with the keys of `A` and other content in every table the service fills (names, defaults, DSP fields): what `A` leaves behind after its unload shows
    /// as a difference to what `a2` leaves.
    pub a2: Module,
    /// The link words the DSP reads to run a row (`(window, word, off)`: the pc of an effect row, the type of a voice row, and the value that turns the row off): a
    /// release must send the word that turns the row off. The words other than these are the content of a row, which stays in the DSP memory until the next module's row overwrites it.
    pub enable: &'static [(u32, u32, u32)],
    /// The service keeps the DSP rows of an absent module (a placeholder: the row's link word stays on, the row is disabled by other means), after every unload and not only
    /// during a reload (docs/plans/extension-model.md E5d), so a load takes the same row over; `false`: it frees them at the unload and the load takes a free row again. The
    /// `enable` words show which. A service with `true` also names the parked bytes of the DSP state in `kept_dsp`; the cases that compare rows by position skip what the parked rows move.
    pub parks_rows: bool,
    /// `(core, address, value)`: whether a word of firmware memory may hold `value` after a release although the start state holds another (the service's documented
    /// free values, such as the empty string a released row points to). The value is part of the question: a word that holds anything else, such as the value `A`
    /// wrote, is a leftover.
    pub kept: fn(&FakeCore, u32, u32) -> bool,
    /// The same for the DSP state: `(byte offset in DspState, value)` pairs a release leaves other than at the start, on purpose. Each entry has a reason in the fixture.
    pub kept_dsp: fn(&FakeCore, usize, u8) -> bool,
    /// The service's row tables (`Rows`), snapshotted with the DSP state and compared the same way.
    pub rows: Rows,
    /// (placeholder voice rows, numbers the service holds for absent modules) now, for a service with `parks_rows` (`None` otherwise). A placeholder row exists only for a held
    /// number, so every snapshot checks `rows <= held`; the cases that unload a module check that its rows are there.
    pub absent_rows: Option<fn(&FakeCore) -> (usize, usize)>,
    /// What the service does while a module runs that the loader does not do: queue live values for the module's rows, as a knob turn does (`None`: the service
    /// queues none). Case 7 calls it with the module loaded, so a release that leaves a value queued on the link shows.
    pub live: Option<fn(&FakeCore)>,
    /// The areas (`memory`, `dsp`, `settings`, `state`, and `survives` when the fixture has it) in which loaded `A` and loaded `a2` may be equal, each with its reason. Every other area must differ, or the
    /// comparison of case 7 cannot see a leftover there.
    pub salt_same: &'static [(&'static str, &'static str)],
    /// The service's data that outlives a module's unload on purpose (the project service keeps what modules put into the current project: it is the project's, not the
    /// module's), as words, `None` when the service keeps none. It is left out of every "back at the start" comparison and is the area `survives` when `A` and `a2` are
    /// compared (the area `state` is `Fixture::state` alone), so a module that fills it makes the two differ. Each use has its reason in the fixture.
    pub survives: Option<fn(&FakeCore) -> Vec<u32>>,
    /// A module the service must refuse at its commit (a record it checks, or a builder call it refuses).
    pub bad: Module,
    /// A module the service refuses while it applies the records, after the rows of earlier records were written (`None`: the service has no such path; its
    /// `check` covers every limit its `apply` has).
    pub bad_late: Option<Late>,
    /// The service's `validate_records` refuses some records (it is not `accepts_records!()`): case 1b then also checks that it refuses a fixture record whose body is one byte short.
    pub rules: bool,
    /// The values the user turned on the module's rows, which the service keeps while the module is gone (`None`: the service keeps none). Case 13.
    pub parked_knobs: Option<ParkedKnobs>,
}

/// What case 13 needs, shaped like the fx service's knob values (`None` for a service that parks none): the firmware keeps the user's value of a module's row in a kit object (the
/// effect's knob array in the live kit), the service parks it when the module unloads for a reload and puts it back when the same module loads into the row again, and only while no kit
/// was loaded or initialised since (the firmware's kit-load hook counts them). The hooks give the case a live kit, the value of the first row's first knob, and a kit load.
#[derive(Clone, Copy)]
pub struct ParkedKnobs {
    /// Adds the live kit to the image after `Fixture::image` (the cell that holds the kit pointer).
    pub image: fn(&mut FakeCore),
    /// Makes kit `n` (0 or 1) the live kit: the two kits have other pointers.
    pub kit: fn(&FakeCore, u32),
    /// The stored value of the loaded module's first row in the live kit, and its write (the user's knob turn).
    pub get: fn(&FakeCore) -> i32,
    pub set: fn(&FakeCore, i32),
    /// A kit load ran in the firmware (what the firmware's kit hook counts), and its undo for the next test.
    pub kit_loaded: fn(&FakeCore),
    pub kit_reset: fn(),
}

/// A refusal during apply and its reference.
pub struct Late {
    /// The module the service refuses while it applies its records.
    pub bad: Module,
    /// A module that claims the same DSP rows and writes the same rows as `bad` before its refusal, and loads: its unload is what the undo of `bad` must equal.
    pub same_but_ok: Module,
    /// Whether the module also fits beside module B (the DSP rows it claims are free with B loaded). The suite runs the refusal beside B too then.
    pub beside_b: bool,
}

/// What a service shows from outside: the firmware words it wrote, the DSP state and the words on the link, its settings, its capacity counts and its own view.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
    mem: BTreeMap<u32, u32>,
    dsp: Vec<u8>,
    /// The bytes of the service's own row tables (`Fixture::rows`).
    rows: Vec<u8>,
    /// The last word written to each (window, word) of the link, where it is not 0 (a word that was never written is 0 in the DSP as well).
    link: BTreeMap<(u32, u32), u32>,
    /// The live values the link queue holds (window, word, value, not yet taken) and the requests queued or in flight.
    live: Vec<(u32, u32, u32, bool)>,
    reqs: usize,
    /// The last word on the link at each of `Fixture::enable` (0: none).
    enable: Vec<u32>,
    settings: Vec<(Vec<u8>, Vec<u8>)>,
    caps: Vec<(u8, u32, u32)>,
    state: Vec<u32>,
    /// What the service keeps on purpose after a module's unload (`Fixture::survives`); never part of a comparison between two snapshots (its `PartialEq` is always true).
    survives: Survivors,
}

/// The view of the data that outlives an unload on purpose. It is shown by `differing_areas` (as its own area, `survives`) and by case 7's check that `a2` differs from `A`,
/// and ignored by every equality, so that "the state is back at its start" holds without it.
#[derive(Clone, Debug, Default)]
struct Survivors(Vec<u32>);

impl PartialEq for Survivors {
    fn eq(&self, _: &Survivors) -> bool {
        true
    }
}

impl Snapshot {
    /// The snapshot with each MISSING placeholder row of the service's row tables as a free row (`Rows::without_placeholders`).
    pub fn without_placeholders(&self, rows: &Rows) -> Snapshot {
        let mut out = self.clone();
        out.rows = (rows.without_placeholders)(&self.rows);
        out
    }

    /// `without_placeholders`, and the rows in the order of their types (`Rows::by_type`).
    pub fn by_type(&self, rows: &Rows) -> Snapshot {
        let mut out = self.without_placeholders(rows);
        out.rows = (rows.by_type)(&out.rows);
        out
    }

    /// Rows that are MISSING placeholders in this snapshot.
    fn placeholder_rows(&self, rows: &Rows) -> usize {
        (rows.placeholder_rows)(&self.rows)
    }

    fn small(&self) -> (Vec<(u8, u32, u32)>, Vec<u32>) {
        (self.caps.clone(), self.state.clone())
    }

    /// What `self` wrote and `other` does not hold any more: every word, link word and DSP state byte that is not 0 in `self` and has another value in `other`.
    /// `other` may have more (rows a module cleared hold the service's free values, rows that were never used hold 0). Each area lists its first 8.
    pub fn lost_in(
        &self,
        other: &Snapshot,
        kept_dsp: &dyn Fn(usize, u8) -> bool,
        kept_rows: &dyn Fn(usize, u8) -> bool,
    ) -> String {
        let mut out = String::new();
        let mut n = 0;
        for (k, v) in &self.mem {
            let o = other.mem.get(k).copied().unwrap_or(0);
            if o != *v && n < 8 {
                out.push_str(&std::format!("  mem {k:x}: {v:#x} -> {o:#x}\n"));
                n += 1;
            }
        }
        let mut n = 0;
        for (k, v) in &self.link {
            let o = other.link.get(k).copied().unwrap_or(0);
            if *v != 0 && o != *v && n < 8 {
                out.push_str(&std::format!("  link {k:x?}: {v:#x} -> {o:#x}\n"));
                n += 1;
            }
        }
        let mut n = 0;
        for (i, b) in self.dsp.iter().enumerate() {
            if *b != 0 && other.dsp[i] != *b && !kept_dsp(i, other.dsp[i]) && n < 8 {
                out.push_str(&std::format!(
                    "  dsp state byte {i}: {b:#x} -> {:#x}\n",
                    other.dsp[i]
                ));
                n += 1;
            }
        }
        let mut n = 0;
        for (i, b) in self.rows.iter().enumerate() {
            if *b != 0 && other.rows[i] != *b && !kept_rows(i, other.rows[i]) && n < 8 {
                out.push_str(&std::format!(
                    "  row table byte {i}: {b:#x} -> {:#x}\n",
                    other.rows[i]
                ));
                n += 1;
            }
        }
        if self.settings != other.settings {
            out.push_str("  settings differ\n");
        }
        if self.state != other.state {
            out.push_str(&std::format!(
                "  state {:x?} -> {:x?}\n",
                self.state,
                other.state
            ));
        }
        out
    }

    /// What an unload leaves of the start state. `self` is the state with the module loaded, `start` the state before the load, `end` the state after the unload.
    /// Two rules, each over memory words, DSP state bytes and settings; the exceptions are `kept` (memory addresses) and `kept_dsp` (DSP state byte offsets), never
    /// the words the unload wrote:
    /// - a word that the load changed and that holds the loaded value after the unload is a leftover (this includes an unload that writes the module's own value back);
    /// - a word that differs from `start` after the unload is a leftover (a constant such as an active flag, or another value).
    ///
    /// Settings are compared whole. Lists the first 8 of each area.
    pub fn survivors(
        &self,
        start: &Snapshot,
        end: &Snapshot,
        kept: &dyn Fn(u32, u32) -> bool,
        kept_dsp: &dyn Fn(usize, u8) -> bool,
        kept_rows: &dyn Fn(usize, u8) -> bool,
    ) -> String {
        let mut out = String::new();
        let mut n = 0;
        let keys: BTreeSet<u32> = self
            .mem
            .keys()
            .chain(start.mem.keys())
            .chain(end.mem.keys())
            .copied()
            .collect();
        for k in keys {
            let get = |s: &Snapshot| s.mem.get(&k).copied().unwrap_or(0);
            let (l, s, e) = (get(self), get(start), get(end));
            if e != s && !kept(k, e) && n < 8 {
                let why = if e == l {
                    "the load wrote it, the unload left it"
                } else {
                    "not the start value"
                };
                out.push_str(&std::format!(
                    "  mem {k:x}: start {s:#x}, loaded {l:#x}, after the unload {e:#x} ({why})\n"
                ));
                n += 1;
            }
        }
        let mut n = 0;
        for i in 0..end.dsp.len() {
            if end.dsp[i] != start.dsp[i] && !kept_dsp(i, end.dsp[i]) && n < 8 {
                out.push_str(&std::format!(
                    "  dsp state byte {i}: start {:#x}, loaded {:#x}, after the unload {:#x}\n",
                    start.dsp[i],
                    self.dsp[i],
                    end.dsp[i]
                ));
                n += 1;
            }
        }
        let mut n = 0;
        for i in 0..end.rows.len() {
            if end.rows[i] != start.rows[i] && !kept_rows(i, end.rows[i]) && n < 8 {
                out.push_str(&std::format!(
                    "  row table byte {i}: start {:#x}, loaded {:#x}, after the unload {:#x}\n",
                    start.rows[i],
                    self.rows[i],
                    end.rows[i]
                ));
                n += 1;
            }
        }
        if end.settings != start.settings {
            out.push_str("  settings after the unload differ from the start\n");
        }
        out
    }

    /// The areas in which two snapshots differ: memory, DSP state, settings, the service's state, and what survives an unload on purpose.
    pub fn differing_areas(&self, other: &Snapshot) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.mem != other.mem {
            v.push("memory");
        }
        if self.dsp != other.dsp || self.rows != other.rows {
            v.push("dsp");
        }
        if self.settings != other.settings {
            v.push("settings");
        }
        if self.state != other.state {
            v.push("state");
        }
        if self.survives.0 != other.survives.0 {
            v.push("survives");
        }
        v
    }

    /// The state after a module's unload as another module with the same keys leaves it: memory words, DSP state bytes, the queue of the link and the words that turn
    /// the rows off, settings, capacity counts and the service's view. The last words sent for a row's content are not compared (they differ with the module).
    pub fn same_after_unload(&self, other: &Snapshot, with_enable: bool) -> Result<(), String> {
        let (mut a, mut b) = (self.clone(), other.clone());
        a.link.clear();
        b.link.clear();
        if !with_enable {
            a.enable.clear();
            b.enable.clear();
        }
        if a == b {
            Ok(())
        } else {
            Err(a.diff(&b))
        }
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
                let (x, y) = (
                    a.get(k).copied().unwrap_or(0),
                    b.get(k).copied().unwrap_or(0),
                );
                if x != y && n < 8 {
                    out.push_str(&std::format!("  {what} {k:x?}: {x:#x} -> {y:#x}\n"));
                    n += 1;
                }
            }
        }
        map(&mut out, "mem", &self.mem, &other.mem);
        map(&mut out, "link", &self.link, &other.link);
        if self.enable != other.enable {
            out.push_str(&std::format!(
                "  row enable words on the link {:x?} -> {:x?}\n",
                self.enable,
                other.enable
            ));
        }
        if self.live != other.live || self.reqs != other.reqs {
            out.push_str(&std::format!(
                "  link queue: live {:x?}, {} requests -> live {:x?}, {} requests\n",
                self.live,
                self.reqs,
                other.live,
                other.reqs
            ));
        }
        if self.dsp != other.dsp {
            let at = (0..self.dsp.len())
                .find(|i| self.dsp[*i] != other.dsp[*i])
                .unwrap_or(0);
            out.push_str(&std::format!("  dsp state differs from byte {at}\n"));
        }
        if self.rows != other.rows {
            let at = (0..self.rows.len())
                .find(|i| self.rows[*i] != other.rows[*i])
                .unwrap_or(0);
            out.push_str(&std::format!("  row tables differ from byte {at}\n"));
        }
        if self.settings != other.settings {
            out.push_str(&std::format!(
                "  settings: {} -> {} entries\n",
                self.settings.len(),
                other.settings.len()
            ));
        }
        if self.caps != other.caps {
            out.push_str(&std::format!(
                "  caps {:?} -> {:?}\n",
                self.caps,
                other.caps
            ));
        }
        if self.state != other.state {
            out.push_str(&std::format!(
                "  state {:x?} -> {:x?}\n",
                self.state,
                other.state
            ));
        }
        out
    }
}

/// The registry check of a load, with what it did to the core.
pub struct CheckRun {
    before: Snapshot,
    after: Snapshot,
    calls: usize,
    result: i32,
}

/// A fake core with one service loaded the way the loader loads its modules.
pub struct Lifecycle<'f, S: Service<FakeCore>> {
    pub core: FakeCore,
    f: &'f Fixture,
    /// How many records of the last load were applied (and written) before the walk ended.
    applied: core::cell::Cell<u32>,
    /// The module records that are loaded now (bit `rec`): what the core's record table says is used when it checks the services' tables.
    used: core::cell::Cell<u32>,
    /// The step of the last load that refused the module (for the failure messages).
    at: core::cell::Cell<&'static str>,
    /// The last run of the registry check inside a load: the state before and after it, the core calls it made and its answer.
    check_run: core::cell::RefCell<Option<CheckRun>>,
    _s: core::marker::PhantomData<S>,
}

/// The names of the check steps, for the failure messages.
const CHECK_STEPS: [&str; 8] = [
    "check COUNTS",
    "check ACTIONS",
    "check NATIVE",
    "check EFFECTS",
    "check DEVICE",
    "check RECORDS",
    "check OWNERS",
    "check ROWS",
];

/// The base address a module's image has in the host runs (any address that has room for the module).
const BASE: u32 = 0x4000;
/// The base of the DSP range: the effect claim computes program counters from it.
const DSP_BASE: u32 = L2_BYTE_BASE + 0x1000;

impl<'f, S: Service<FakeCore>> Lifecycle<'f, S> {
    /// A core of the fixture's profile, and every owner's `remove` once (the service's statics start empty).
    pub fn new(f: &'f Fixture) -> Self {
        Self::with_image(f, |_| {})
    }

    /// `new` with `extra` applied to the core after the fixture's image, before the service's first `remove` and settle.
    pub fn with_image(f: &'f Fixture, extra: impl FnOnce(&mut FakeCore)) -> Self {
        // a core starts with the service's row tables as linked (zero), as it starts with a zero `DspState`
        (f.rows.reset)();
        let mut core = FakeCore::new();
        core.device = f.device;
        (f.image)(&mut core);
        extra(&mut core);
        let l = Lifecycle {
            core,
            f,
            applied: core::cell::Cell::new(0),
            used: core::cell::Cell::new(0),
            at: core::cell::Cell::new(""),
            check_run: core::cell::RefCell::new(None),
            _s: core::marker::PhantomData,
        };
        for owner in 1..=MAX_MODULES as u8 {
            unsafe { S::remove(&l.core, owner) };
        }
        // a service that keeps DSP-side state per record (the samples client slots) frees it in `dsp_release`, which `remove` does not reach
        for rec in 0..MAX_MODULES {
            unsafe { S::dsp_release(&l.core, rec) };
        }
        // the boot: the services settle once over the keys of the selection, before the first module loads
        let mut sel = SettleCtx::new();
        let mut mods = std::vec![&f.a, &f.b, &f.bad];
        mods.extend(f.bad_late.as_ref().map(|l| &l.bad));
        for (i, m) in mods.iter().enumerate() {
            let k = (m.keys)();
            sel.count = i + 1;
            l.core.set_keys(
                i as u32,
                dkrt_sdk::records::KIND_MACHINE,
                &k.mk[..k.nm.min(8)],
            );
            for (kind, keys) in m.groups {
                l.core.set_keys(i as u32, *kind, keys);
            }
        }
        unsafe { S::settle(&l.core, &sel) };
        l
    }

    /// Run `Service::settle` again over the keys of `mods`, the modules that are loaded now, in selection order: what a reload does before it loads the new file.
    pub fn settle_over(&self, mods: &[&Module]) {
        let mut sel = SettleCtx::new();
        for (i, m) in mods.iter().enumerate() {
            let k = (m.keys)();
            sel.count = i + 1;
            self.core.set_keys(
                i as u32,
                dkrt_sdk::records::KIND_MACHINE,
                &k.mk[..k.nm.min(8)],
            );
            for (kind, keys) in m.groups {
                self.core.set_keys(i as u32, *kind, keys);
            }
        }
        unsafe { S::settle(&self.core, &sel) };
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
        let mut live = core.link.borrow().live_words();
        live.sort();
        let reqs = core.link.borrow().requests_in_use();
        let mut settings = core.settings.borrow().clone();
        settings.sort();
        let mut caps = Vec::new();
        for kind in 0..32u8 {
            if let Some((l, u)) = unsafe { S::cap(core, kind) } {
                caps.push((kind, l, u));
            }
        }
        if let Some(absent) = self.f.absent_rows {
            let (rows, held) = absent(core);
            assert!(
                rows <= held,
                "{}: {rows} placeholder rows for {held} numbers held for absent modules",
                self.f.name
            );
        }
        Snapshot {
            mem: core
                .mem
                .borrow()
                .iter()
                .filter(|(_, v)| **v != 0)
                .map(|(k, v)| (*k, *v))
                .collect(),
            dsp,
            rows: (self.f.rows.bytes)(),
            live,
            reqs,
            enable: self
                .f
                .enable
                .iter()
                .map(|k| link.get(&(k.0, k.1)).copied().unwrap_or(0))
                .collect(),
            link,
            settings,
            caps,
            state: (self.f.state)(core),
            survives: Survivors(self.f.survives.map_or_else(Vec::new, |f| f(core))),
        }
    }

    /// The core's check of the services' tables (invariant 3, `Service::table_check`) with the records that are loaded now: the loader runs it at the end of every load
    /// attempt and every unload, and so does the suite. A mismatch fails the case.
    fn tables_agree(&self, after: &str) {
        let mut fault = TableFault::default();
        let r = unsafe { S::table_check(&self.core, self.used.get(), &mut fault) };
        assert_eq!(
            r,
            0,
            "{} ({}): the service's tables disagree with the loaded records after {after}: {fault:?}",
            self.f.name,
            profile(self.f)
        );
    }

    /// `S::declare`: the kinds it names.
    pub fn declared(&self) -> Declare {
        let mut d = Declare::new();
        unsafe { S::declare(&mut d) };
        d
    }

    fn job(&self, m: &Module) -> (Job, Vec<u8>, Vec<u8>, [u8; 32]) {
        let mut fxs = std::vec![0u8; FX_REC as usize * m.dsp.fxs as usize];
        let mut voices = std::vec![0u8; VOICE_V3 as usize * m.dsp.voices as usize];
        let keys = (m.keys)();
        let salt = m.dsp.salt;
        for i in 0..m.dsp.voices as usize {
            let e = VOICE_V3 as usize * i;
            let key = keys.mk.get(i).copied().unwrap_or(0);
            let mut put =
                |at: u32, v: u32| voices[e + at as usize..][..4].copy_from_slice(&v.to_be_bytes());
            put(VOICE_TYPE, key);
            put(VOICE_ENTRY, 0x40 + 4 * salt);
            if salt != 0 {
                put(VOICE_NOTE_ON, 0x80 + 4 * salt);
                put(VOICE_STATE_BYTES, 16 * salt);
                put(VOICE_CLEAR_BYTES, 8 * salt);
                put(VOICE_VOICES, 1);
                put(VOICE_LATCH, salt);
                for k in 0..8 {
                    put(
                        VOICE_V3_RANGES + 4 * k,
                        salt * (k + 1) | (salt * (k + 2)) << 16,
                    );
                }
            }
        }
        for i in 0..m.dsp.fxs as usize {
            let e = FX_REC as usize * i;
            let mut put =
                |at: u32, v: u32| fxs[e + at as usize..][..4].copy_from_slice(&v.to_be_bytes());
            if salt != 0 {
                put(FX_PROCESS, 0x100 + 2 * salt);
                put(FX_DESC, 0x200 + 4 * salt);
                put(FX_STATE, 16 * salt + 8);
                for k in 0..8 {
                    put(FX_PARAMS + 4 * k, salt * (k + 1));
                }
            }
        }
        let mut j = Job::empty();
        j.fxs = fxs.as_mut_ptr();
        j.nfx = m.dsp.fxs;
        j.voices = voices.as_mut_ptr();
        j.nvoice = m.dsp.voices;
        j.vstride = VOICE_V3;
        // the track-table record of a DSP section (little-endian words): kind 2, the local id, the offsets of its records and control sets
        let mut bank = [0u8; 32];
        if let Some(local) = m.dsp.bank {
            for (i, w) in [2u32, local, 0x100, 0x200].iter().enumerate() {
                bank[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
        (j, fxs, voices, bank)
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
        self.at.set("init");
        if init.err != 0 {
            return Err(init.err);
        }
        self.at.set("commit, prepare");
        let mut sink = RecordSink::measure();
        let r = S::module_commit(
            core,
            rec,
            &sink_ctx(commit_phase::PREPARE, &mut sink, &keys),
        );
        if r != OK {
            return Err(r);
        }
        generic(&mut sink);
        self.at.set("commit, write");
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
            self.at.set("registry check");
            let before = self.snapshot();
            let calls = core.calls().len();
            let r = self.walk_check(&buf);
            *self.check_run.borrow_mut() = Some(CheckRun {
                before,
                after: self.snapshot(),
                calls: core.calls().len() - calls,
                result: r,
            });
            if r != OK {
                return Err(r);
            }
        }
        self.at.set("commit, declare");
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
            self.at.set("apply");
            self.applied.set(0);
            let mut r = S::apply(core, &c);
            if r == OK {
                each(buf.as_ptr(), |kind, b| {
                    if r == OK {
                        r = S::apply_record(core, kind, b, &c);
                        if r == OK {
                            self.applied.set(self.applied.get() + 1);
                        }
                    }
                });
            }
            if r != OK {
                return Err(r);
            }
        }
        Ok(buf)
    }

    /// The registry walker's check of a section for this one service: the layout (`check_layout`, the walker's own function) against the kinds of the fixture, then the
    /// service's check steps in order.
    pub fn walk_check(&self, sec: &[u8]) -> i32 {
        let r = unsafe {
            check_layout(sec.as_ptr(), sec.len() as u32, self.f.device, |kind| {
                self.f.kinds.iter().find(|k| k.0 == kind).map(|k| k.1)
            })
        };
        if r != OK {
            return r;
        }
        let c = CheckCtx {
            reg: sec.as_ptr(),
            image_size: 0x4000,
        };
        for step in 0..check_step::COUNT {
            let r = unsafe { S::check(&self.core, step, &c) };
            if r != OK {
                self.at.set(
                    CHECK_STEPS
                        .get(step as usize)
                        .copied()
                        .unwrap_or("registry check"),
                );
                return r;
            }
        }
        OK
    }

    /// Load module `m` into record `rec`; the loader's steps. A refusal undoes what was done, as the loader's `fail` does, and is returned.
    pub fn load(&self, rec: usize, m: &Module) -> i32 {
        self.load_staged(rec, m).map(|_| OK).unwrap_or_else(|e| e)
    }

    /// The DSP claim of a load and nothing else: the rows `m` takes are claimed (as `dsp_claim` does for the DSP loader), none is published.
    pub fn claim_only(&self, rec: usize, m: &Module) {
        let core = &self.core;
        core.boot_index.set(m.sel);
        unsafe {
            let (mut job, _fxs, _voices, bank) = self.job(m);
            if m.dsp.bank.is_some() {
                job.bank = bank.as_ptr();
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
        core.boot_index.set(u32::MAX);
    }

    /// `load`, giving the section the walker checked.
    pub fn load_staged(&self, rec: usize, m: &Module) -> Result<Vec<u8>, i32> {
        let core = &self.core;
        core.boot_index.set(m.sel);
        let r = self.load_inner(rec, m);
        core.boot_index.set(u32::MAX);
        if r.is_ok() {
            self.used.set(self.used.get() | 1 << rec);
        }
        self.tables_agree(if r.is_ok() {
            "a load"
        } else {
            "a refused load"
        });
        r
    }

    fn load_inner(&self, rec: usize, m: &Module) -> Result<Vec<u8>, i32> {
        let core = &self.core;
        self.applied.set(0);
        let owner = rec as u8 + 1;
        unsafe {
            let (mut job, _fxs, _voices, bank) = self.job(m);
            if m.dsp.bank.is_some() {
                job.bank = bank.as_ptr();
            }
            let dsp = m.dsp.any();
            if dsp {
                let r = S::dsp_admit(core, &job);
                if r != OK {
                    self.at.set("dsp admit");
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
            // an `init` may set what a firmware call answers (a module whose native page registration fails); it lasts for this load
            let staged = self.stage(rec, m, &init);
            core.fw_result.set(0);
            match staged {
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
        self.used.set(self.used.get() & !(1 << rec));
        self.tables_agree("an unload");
    }

    /// At an idle point (the loader idle, the boot over, no load pending) a service that parks the rows of absent modules has one placeholder for each number it holds that no
    /// module has: one pass of its tick, then the placeholders equal those numbers (`Fixture::absent_rows`).
    pub fn expect_all_parked(&self) {
        let Some(absent) = self.f.absent_rows else {
            return;
        };
        unsafe { S::tick(&self.core, dkrt_abi::service::tick_phase::LAST) };
        let (rows, held) = absent(&self.core);
        assert_eq!(
            rows, held,
            "{} ({}): placeholder rows against the numbers held for absent modules at an idle point",
            self.f.name,
            profile(self.f)
        );
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
        panic!("{} ({}): {what}\n{}", f.name, profile(f), a.diff(b));
    }
}

/// The lock of the service statics: a test that loads modules through `Lifecycle` outside `contract` takes it for its length.
pub fn locked() -> MutexGuard<'static, ()> {
    SUITE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The six cases of the contract (docs/plans/extension-model.md 3.5) for service `S` on the profile of `f`.
pub fn contract<S: Service<FakeCore>>(f: &Fixture) {
    let _g = locked();
    declares_its_kinds::<S>(f);
    validates_its_fixtures_records::<S>(f);
    check_writes_nothing::<S>(f);
    reload_equivalence::<S>(f);
    another_module_is_untouched::<S>(f);
    a_refused_commit_leaves_nothing::<S>(f);
    reload_ten_times::<S>(f);
    leaves_no_residue::<S>(f);
    a_refusal_during_apply_leaves_nothing::<S>(f);
    settle_over_the_live_set_changes_nothing::<S>(f);
    teardown_refusal_leaves_no_residue::<S>(f);
    a_reload_that_ends_leaves_no_placeholder::<S>(f);
    a_live_unload_and_load_keeps_the_numbers::<S>(f);
    a_reload_takes_over_the_parked_row_and_a_failed_one_leaves_the_gap_row::<S>(f);
    parked_values_belong_to_the_module_and_kit_that_left::<S>(f);
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

/// The records of kind `kind` in section `sec` (a module's committed block after its 16-byte header): their bodies, in file order. A record shorter than its 8-byte header, or
/// past the end of the section, is a defect of the section and fails the case instead of looping.
fn bodies_of(sec: &[u8], kind: u32) -> Vec<&[u8]> {
    let mut bodies: Vec<&[u8]> = Vec::new();
    let mut at = RecordSink::HEADER;
    while at + 8 <= sec.len() {
        let word = |o: usize| u32::from_be_bytes(sec[o..o + 4].try_into().unwrap());
        let len = word(at + 4) as usize;
        assert!(
            len >= 8 && at + len <= sec.len(),
            "the section has a record of length {len} at {at}"
        );
        if word(at) == kind {
            bodies.push(&sec[at + 8..at + len]);
        }
        at += len;
    }
    bodies
}

/// Case 1b: `validate_records` accepts the records the fixture's modules produce. The section a module commits is split by record kind, and each kind the service
/// owns goes through the validator as a set, as `patchkit records --check` does for a built module. A service that owns kinds must see records of at least one of them in
/// each module (else the case proves nothing). A service with rules (`Fixture::rules`) refuses at least one kind when its first record's body is one byte short.
fn validates_its_fixtures_records<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    for (what, m, rec) in [("A", &f.a, 0), ("B", &f.b, 1)] {
        let sec = l
            .load_staged(rec, m)
            .unwrap_or_else(|e| panic!("{}: module {what} is refused with {e}", f.name));
        let mut with_records = 0;
        let mut refused = 0;
        for &(kind, _) in f.kinds {
            let bodies = bodies_of(&sec, kind);
            if bodies.is_empty() {
                continue;
            }
            with_records += 1;
            let mut problems = Vec::new();
            S::validate_records(kind, &bodies, &mut |p| problems.push(p.to_string()));
            assert!(
                problems.is_empty(),
                "{}: module {what}: validate_records refuses the records the module commits: {problems:?}",
                f.name
            );
            if f.rules {
                let mut short = bodies.clone();
                short[0] = &short[0][..short[0].len() - 1];
                S::validate_records(kind, &short, &mut |_| refused += 1);
            }
        }
        assert!(
            !f.rules || refused > 0,
            "{}: module {what}: validate_records accepts every kind with its first record one byte short",
            f.name
        );
        assert!(
            f.kinds.is_empty() || with_records > 0,
            "{}: module {what} commits no record of the kinds the service owns, so validate_records saw none",
            f.name
        );
    }
}

/// Case 2: `check` writes nothing. The registry check inside the load of each module runs in the state the loader checks in (after the commit, before the apply);
/// the state before and after it is the same, it makes no core call, and it goes through every step (the module loads). A second run over the section of a module
/// that is loaded is compared the same way (it may refuse there, as the module's ids are taken).
fn check_writes_nothing<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    for (what, m, rec) in [("A", &f.a, 0), ("B", &f.b, 1)] {
        let sec = l
            .load_staged(rec, m)
            .unwrap_or_else(|e| panic!("{}: module {what} is refused with {e}", f.name));
        if sec.len() > RecordSink::HEADER {
            let run = l
                .check_run
                .borrow_mut()
                .take()
                .expect("a section is checked");
            assert_eq!(run.result, OK, "{}: module {what}: check", f.name);
            assert_eq!(
                run.calls, 0,
                "{}: check made core calls (module {what})",
                f.name
            );
            expect(
                &std::format!("check wrote to the core (during the load of {what})"),
                f,
                &run.before,
                &run.after,
            );
        }
        let before = l.snapshot();
        let calls_before = l.core.calls().len();
        let _ = l.check_only(&sec);
        expect(
            &std::format!("check wrote to the core (module {what} loaded)"),
            f,
            &before,
            &l.snapshot(),
        );
        assert_eq!(
            l.core.calls().len(),
            calls_before,
            "{}: check made calls (module {what} loaded)",
            f.name
        );
    }
    l.unload(0);
    l.unload(1);
}

/// Case 3: the state after `A` is the state after `A`, unload `A`, `A`; what `B` alone writes is the same after `A`, unload `A`, `B` (and the capacity counts and
/// the service's view are `B`'s); the capacity counts and the service's view return to their start after the unload.
fn reload_equivalence<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let after_a = l.snapshot();
    l.unload(0);
    expect_small("after unload A", f, &start, &l.snapshot());
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: module A is refused after its unload",
        f.name
    );
    expect("A, unload A, A differs from A", f, &after_a, &l.snapshot());
    l.unload(0);

    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.b), OK, "{}: module B is refused", f.name);
    let after_b = l.snapshot();
    l.unload(0);
    assert_eq!(l.load(0, &f.a), OK);
    l.unload(0);
    assert_eq!(
        l.load(0, &f.b),
        OK,
        "{}: module B is refused after A",
        f.name
    );
    // what `A` held and released stays as the service's free values (a machine row is `A`'s by its key, whatever loads next), which `B` alone never wrote: `B`'s own
    // words, DSP state and ids are what is compared, and the capacity counts
    // (a service that parks the rows of an absent module keeps A's row, so B has the next one: B's words are in another row and are not compared)
    if !f.parks_rows {
        let lost = after_b.by_type(&f.rows).lost_in(
            &l.snapshot().by_type(&f.rows),
            &|o, v| (f.kept_dsp)(&l.core, o, v),
            &|o, v| (f.rows.kept)(&l.core, o, v),
        );
        assert!(
            lost.is_empty(),
            "{} ({}): A, unload A, B differs from B\n{lost}",
            f.name,
            profile(f)
        );
    }
    expect_small("A, unload A, B", f, &after_b, &l.snapshot());
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
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: module A is refused beside B",
        f.name
    );
    l.unload(0);
    let lost = b_alone.lost_in(
        &l.snapshot(),
        &|o, v| (f.kept_dsp)(&l.core, o, v),
        &|o, v| (f.rows.kept)(&l.core, o, v),
    );
    assert!(
        lost.is_empty(),
        "{} ({}): A's unload changed what B holds\n{lost}",
        f.name,
        profile(f)
    );
    let once = l.snapshot();
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: module A is refused beside B again",
        f.name
    );
    l.unload(0);
    expect(
        "a second cycle of A beside B differs from the first",
        f,
        &once,
        &l.snapshot(),
    );
    l.unload(1);

    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK);
    assert_eq!(
        l.load(1, &f.b),
        OK,
        "{}: module B is refused beside A",
        f.name
    );
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
    assert_ne!(
        r, OK,
        "{}: the module that must be refused is accepted",
        f.name
    );
    expect("a refused commit left something", f, &before, &l.snapshot());
    // with a module loaded the refusal of another leaves that module alone
    assert_eq!(l.load(1, &f.b), OK);
    assert_eq!(l.load(0, &f.a), OK);
    l.unload(0);
    let with_b = l.snapshot();
    assert_ne!(l.load(0, &f.bad), OK);
    expect(
        "a refused commit changed a loaded module's state",
        f,
        &with_b,
        &l.snapshot(),
    );
    l.unload(1);
}

/// Case 9: `settle` is idempotent. A reload runs it again over the modules that are loaded (the new file's keys are in the old one's slot); with modules A and B
/// loaded, one run or two over the live set leave everything the service holds as it was, so the numbers of loaded modules do not move.
fn settle_over_the_live_set_changes_nothing<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    assert_eq!(l.load(1, &f.b), OK, "{}: module B is refused", f.name);
    let before = l.snapshot();
    for run in 0..2 {
        l.settle_over(&[&f.a, &f.b]);
        expect(
            &std::format!("settle over the live set (run {run}) changed what the service holds"),
            f,
            &before,
            &l.snapshot(),
        );
    }
    l.unload(1);
    l.unload(0);
}

/// Case 10: `teardown_refusal` of a module that is loaded is `OK` or `E_RELOAD_ROUTED` (a module's own state, never a refusal for another record), and once the
/// module is unloaded it is `OK`: an unload leaves nothing that holds up the reload of the record's next module.
fn teardown_refusal_leaves_no_residue<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let r = unsafe { S::teardown_refusal(&l.core, 1) };
    assert!(
        r == OK || r == dkrt_sdk::E_RELOAD_ROUTED,
        "{}: teardown_refusal answers {r} for a loaded module",
        f.name
    );
    assert_eq!(
        unsafe { S::teardown_refusal(&l.core, 2) },
        OK,
        "{}: teardown_refusal refuses record 1, which holds no module",
        f.name
    );
    l.unload(0);
    assert_eq!(
        unsafe { S::teardown_refusal(&l.core, 1) },
        OK,
        "{}: teardown_refusal still refuses after the unload",
        f.name
    );
}

/// Case 11: what a service parks during a reload (a placeholder for the module's numbers) is, when the reload ends, what a plain unload leaves, in both ways it can end:
/// the new version loads (it takes the placeholder over) or it does not (`live_done` keeps the placeholder of a number the service holds: the module is absent, E5d). Both
/// leave the capacity counts and the service's view as they were at the start, and the DSP state but for the bytes `kept_dsp` names (the parked rows), so a row that leaks
/// fails here. The memory words of an unload hold the placeholder values (`kept` names them where a fixture has any), and are not compared.
fn a_reload_that_ends_leaves_no_placeholder<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    // the new version loads: A, then the reload of A
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    l.core.live_op.set(1);
    l.unload(0);
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: the reload of A is refused",
        f.name
    );
    unsafe { S::live_done(&l.core) };
    l.core.live_op.set(0);
    l.unload(0);
    let end = l.snapshot();
    expect_small("A, its reload, unload A", f, &start, &end);
    assert!(
        dsp_same(f, &l.core, &start, &end),
        "{} ({}): the DSP state after a reload and an unload differs from the start",
        f.name,
        profile(f)
    );
    // the new version does not load
    assert_eq!(l.load(0, &f.a), OK);
    l.core.live_op.set(1);
    l.unload(0);
    unsafe { S::live_done(&l.core) };
    l.core.live_op.set(0);
    let end = l.snapshot();
    expect_small("A, a reload that fails", f, &start, &end);
    assert!(
        dsp_same(f, &l.core, &start, &end),
        "{} ({}): the DSP state after a failed reload differs from the start",
        f.name,
        profile(f)
    );
    // the module is not loaded, as after a plain unload: the firmware words are those a plain unload leaves (a machine row is MISSING in both)
    let failed = l.snapshot();
    let plain = Lifecycle::<S>::new(f);
    assert_eq!(plain.load(0, &f.a), OK);
    plain.unload(0);
    let plain = plain.snapshot();
    assert!(
        plain.mem == failed.mem,
        "{} ({}): the memory after a failed reload differs from a plain unload\n{}",
        f.name,
        profile(f),
        plain.diff(&failed)
    );
    l.expect_all_parked();
}

/// Case 12: a live unload of a module and a live load of it again (docs/plans/extension-model.md E5c) leave the service as it was: the job ends an unload with `live_done`
/// and no settle, and a load settles over the live set with the file at its index before it loads. With A and B loaded, take B down and load it again: B gets the numbers
/// it had, A is untouched, and the service holds what it held before. The same for A alone.
fn a_live_unload_and_load_keeps_the_numbers<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    assert_eq!(l.load(1, &f.b), OK, "{}: module B is refused", f.name);
    let both = l.snapshot();
    for round in 0..3 {
        l.unload(1);
        unsafe { S::live_done(&l.core) };
        l.settle_over(&[&f.a, &f.b]);
        assert_eq!(
            l.load(1, &f.b),
            OK,
            "{}: B is refused when loaded again (round {round})",
            f.name
        );
        unsafe { S::live_done(&l.core) };
        expect(
            &std::format!("unload and load of B (round {round}) changed what the service holds"),
            f,
            &both,
            &l.snapshot(),
        );
    }
    l.unload(1);
    l.unload(0);
    // A alone, numbered by a run-time settle (the snapshot after the boot's settle counts every module of the fixture)
    l.settle_over(&[&f.a]);
    assert_eq!(l.load(0, &f.a), OK);
    let alone = l.snapshot();
    l.unload(0);
    unsafe { S::live_done(&l.core) };
    l.settle_over(&[&f.a]);
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: A is refused when loaded again",
        f.name
    );
    unsafe { S::live_done(&l.core) };
    expect(
        "unload and load of A changed what the service holds",
        f,
        &alone,
        &l.snapshot(),
    );
    l.unload(0);
}

/// Case 12b: a reload of module A (the live op set, its unload, its load) takes over what the unload parked: the load gets the same rows and the same words on the link
/// as before, and the service holds what it held with A loaded. Where the service parks DSP rows (`Fixture::parks_rows`), the words that run a row stay on while A is
/// gone; where it does not, they are off. A reload that ends without A (`live_done`) leaves the rows off, as a plain unload does.
fn a_reload_takes_over_the_parked_row_and_a_failed_one_leaves_the_gap_row<S: Service<FakeCore>>(
    f: &Fixture,
) {
    let on = |s: &Snapshot| -> Vec<bool> {
        f.enable
            .iter()
            .zip(&s.enable)
            .map(|(e, w)| *w != 0 && *w != e.2)
            .collect()
    };
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let loaded = l.snapshot();
    assert!(
        f.enable.is_empty() || on(&loaded).contains(&true),
        "{}: the load of A publishes no row: the check of the parked rows sees nothing",
        f.name
    );
    // the reload: A is parked, then taken over
    l.core.live_op.set(1);
    l.unload(0);
    let parked = l.snapshot();
    if f.parks_rows {
        assert_eq!(
            parked.enable,
            loaded.enable,
            "{} ({}): a parked row keeps its words on the link",
            f.name,
            profile(f)
        );
    }
    assert_eq!(
        on(&parked),
        if f.parks_rows {
            on(&loaded)
        } else {
            std::vec![false; f.enable.len()]
        },
        "{} ({}): the rows of A while A is gone (parks_rows {})",
        f.name,
        profile(f),
        f.parks_rows
    );
    assert_eq!(
        l.load(0, &f.a),
        OK,
        "{}: the reload of A is refused",
        f.name
    );
    let again = l.snapshot();
    assert_eq!(
        again.enable,
        loaded.enable,
        "{} ({}): the reload of A runs other rows or types than the first load",
        f.name,
        profile(f)
    );
    expect(
        "the reload of A changed what the service holds",
        f,
        &loaded,
        &again,
    );
    unsafe { S::live_done(&l.core) };
    expect(
        "the end of a reload that took everything over changed what the service holds",
        f,
        &loaded,
        &l.snapshot(),
    );
    l.core.live_op.set(0);
    l.unload(0);
    gap_rows_after_an_unload(f, &on, &loaded, &l.snapshot());
    // a reload that ends without A: what nobody took over is freed
    assert_eq!(l.load(0, &f.a), OK);
    l.core.live_op.set(1);
    l.unload(0);
    unsafe { S::live_done(&l.core) };
    l.core.live_op.set(0);
    gap_rows_after_an_unload(f, &on, &loaded, &l.snapshot());
    l.expect_all_parked();
}

/// What a module's rows are after its unload, or after a reload that ended without it (E5d: the module is absent either way): where the service parks the rows of an
/// absent module (`Fixture::parks_rows`) the words that run a row stay on, as with A loaded (the rows are disabled by other means, which the service's own tests check);
/// where it does not they are off. This is the half of case 12b that says what a gone module's rows are.
fn gap_rows_after_an_unload(
    f: &Fixture,
    on: &dyn Fn(&Snapshot) -> Vec<bool>,
    loaded: &Snapshot,
    s: &Snapshot,
) {
    if f.absent_rows.is_some() {
        let rows = on(loaded).iter().filter(|o| **o).count();
        assert_eq!(
            s.placeholder_rows(&f.rows),
            rows,
            "{} ({}): the rows of A after its unload are placeholders",
            f.name,
            profile(f)
        );
    }
    if f.parks_rows {
        assert_eq!(
            s.enable,
            loaded.enable,
            "{} ({}): the rows of an absent module do not keep their words: {:x?}",
            f.name,
            profile(f),
            s.enable
        );
    } else {
        assert!(
            on(s).iter().all(|o| !o),
            "{} ({}): a row of A still runs after its unload: {:x?}",
            f.name,
            profile(f),
            s.enable
        );
    }
}

/// Case 13 (`Fixture::parked_knobs`): the values the user turned survive a reload of the same module, as the firmware's kit array keeps them, and only that: they are the defaults
/// when another kit is live at the load, when a kit load ran while the module was gone, when another module takes the row, and when a module of another name has the
/// same keys. The user's value is `0x1234`, a word no fixture holds as a default. Docs: docs/plans/extension-model.md, E5b.
fn parked_values_belong_to_the_module_and_kit_that_left<S: Service<FakeCore>>(f: &Fixture) {
    let Some(k) = f.parked_knobs else { return };
    const TURNED: i32 = 0x1234;
    const LEFT: i32 = 0x2222;
    let mut l = Lifecycle::<S>::with_image(f, k.image);
    let (name, turned) = (f.name, "the value the user turned");
    (k.kit)(&l.core, 0);
    assert_eq!(l.load(0, &f.a), OK, "{name}: module A is refused");
    let default = (k.get)(&l.core);
    assert_ne!(
        default, TURNED,
        "{name}: the fixture's default must not be {TURNED:#x}"
    );
    (k.set)(&l.core, TURNED);
    // a reload: the job sets the live op, the teardown runs, the file loads again
    l.core.live_op.set(1);
    assert_eq!(
        unsafe { S::teardown_refusal(&l.core, 1) },
        OK,
        "{name}: a loaded module is refused its teardown"
    );
    l.unload(0);
    l.core.live_op.set(0);
    assert_eq!(l.load(0, &f.a), OK, "{name}: the reload of A is refused");
    assert_eq!(
        (k.get)(&l.core),
        TURNED,
        "{name}: {turned} is lost in a reload of the same module"
    );
    // an unload and a load, with another kit live in between: the defaults
    l.unload(0);
    (k.kit)(&l.core, 1);
    assert_eq!(l.load(0, &f.a), OK);
    assert_eq!(
        (k.get)(&l.core),
        default,
        "{name}: a kit the module never held gets the defaults"
    );
    // an unload, a kit load in the firmware, then the load: the defaults
    (k.set)(&l.core, LEFT);
    l.unload(0);
    (k.kit_loaded)(&l.core);
    assert_eq!(l.load(0, &f.a), OK);
    assert_eq!(
        (k.get)(&l.core),
        default,
        "{name}: a kit was loaded while the module was gone"
    );
    // another module takes the row: its defaults, not the left value
    (k.set)(&l.core, LEFT);
    l.unload(0);
    assert_eq!(l.load(0, &f.b), OK);
    assert_ne!(
        (k.get)(&l.core),
        LEFT,
        "{name}: another module inherits the left value"
    );
    l.unload(0);
    // the same keys in a module of another name: the defaults
    (k.kit_reset)();
    (k.kit)(&l.core, 0);
    assert_eq!(l.load(0, &f.a), OK);
    (k.set)(&l.core, TURNED);
    l.unload(0);
    let own = core::mem::replace(&mut l.core.name, String::from("other\0"));
    assert_eq!(l.load(0, &f.a), OK);
    assert_eq!(
        (k.get)(&l.core),
        default,
        "{name}: a module of another name inherits the values"
    );
    l.unload(0);
    l.core.name = own;
    (k.kit_reset)();
}

/// Whether the DSP state bytes and the row tables of `end` are those of `start`, but for the bytes the fixture's `kept_dsp` and `rows.kept` name.
fn dsp_same(f: &Fixture, core: &FakeCore, start: &Snapshot, end: &Snapshot) -> bool {
    (0..start.dsp.len()).all(|i| start.dsp[i] == end.dsp[i] || (f.kept_dsp)(core, i, end.dsp[i]))
        && (0..start.rows.len())
            .all(|i| start.rows[i] == end.rows[i] || (f.rows.kept)(core, i, end.rows[i]))
}

/// Case 6: case 3 ten times: no table grows and no sticky value drifts.
fn reload_ten_times<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    let first = l.snapshot();
    for i in 0..10 {
        l.unload(0);
        assert_eq!(
            l.load(0, &f.a),
            OK,
            "{}: module A is refused at reload {i}",
            f.name
        );
        expect(
            &std::format!("reload {i} of A drifted"),
            f,
            &first,
            &l.snapshot(),
        );
    }
    l.unload(0);
    assert_eq!(l.load(0, &f.b), OK);
    let first_b = l.snapshot();
    for i in 0..10 {
        l.unload(0);
        assert_eq!(l.load(0, &f.a), OK);
        l.unload(0);
        assert_eq!(
            l.load(0, &f.b),
            OK,
            "{}: module B is refused at round {i}",
            f.name
        );
        expect(
            &std::format!("round {i} of A and B drifted"),
            f,
            &first_b,
            &l.snapshot(),
        );
    }
    l.unload(0);
}

/// Case 7: an unload leaves nothing of the module. The state after "load `A`, unload `A`" (memory words, DSP state, the link's queue and the words that turn the
/// rows off, settings, capacity counts, the service's view) is the same as after "load `a2`, unload `a2`", both from a fresh start: `a2` has the keys of `A` and
/// other content, so whatever a release leaves of a module's content differs between the two. And no memory word that `A`'s load wrote is still at `A`'s value after
/// the unload unless the unload wrote it (its free value) or the fixture names it.
fn leaves_no_residue<S: Service<FakeCore>>(f: &Fixture) {
    let l = Lifecycle::<S>::new(f);
    let start = l.snapshot();
    assert_eq!(l.load(0, &f.a), OK, "{}: module A is refused", f.name);
    if let Some(live) = f.live {
        live(&l.core);
    }
    let loaded_a = l.snapshot();
    l.unload(0);
    let end_a = l.snapshot();
    expect_small("after unload A", f, &start, &end_a);
    // the rows that the module's load published are off after its unload (a row that was never published has no word)
    let on = |s: &Snapshot| {
        f.enable
            .iter()
            .zip(&s.enable)
            .filter(|(e, w)| **w != 0 && **w != e.2)
            .count()
    };
    assert!(
        f.absent_rows.is_none() || end_a.placeholder_rows(&f.rows) == on(&loaded_a),
        "{} ({}): the rows of A after its unload are not parked",
        f.name,
        profile(f)
    );
    // a service that parks the rows of an absent module (`parks_rows`) leaves A's row words on the link as they were: the row is parked, disabled by other means
    assert_eq!(
        on(&end_a),
        if f.parks_rows { on(&loaded_a) } else { 0 },
        "{} ({}): the rows of A after its unload (parks_rows {}): {:x?}",
        f.name,
        profile(f),
        f.parks_rows,
        end_a.enable
    );
    assert!(
        f.live.is_none() || loaded_a.live != start.live,
        "{}: the live hook queues no value: the check of the link queue sees nothing",
        f.name
    );
    assert!(
        f.enable.is_empty() || on(&loaded_a) > 0,
        "{}: the load of A publishes no row: the check of the words that turn a row off sees nothing",
        f.name
    );
    // nothing of the module is queued on the link after its unload
    assert!(
        end_a.live == start.live && end_a.reqs == start.reqs,
        "{} ({}): the unload of A leaves values or requests on the link queue: live {:x?}, {} requests",
        f.name,
        profile(f),
        end_a.live,
        end_a.reqs
    );
    let left = loaded_a.survivors(
        &start,
        &end_a,
        &|a, v| (f.kept)(&l.core, a, v),
        &|o, v| (f.kept_dsp)(&l.core, o, v),
        &|o, v| (f.rows.kept)(&l.core, o, v),
    );
    assert!(
        left.is_empty(),
        "{} ({}): the unload of A left what A wrote\n{left}",
        f.name,
        profile(f)
    );

    let l = Lifecycle::<S>::new(f);
    assert!(l.snapshot() == start, "{}: a fresh start differs", f.name);
    assert_eq!(l.load(0, &f.a2), OK, "{}: module a2 is refused", f.name);
    if let Some(live) = f.live {
        live(&l.core);
    }
    let loaded_a2 = l.snapshot();
    assert!(
        loaded_a.same_after_unload(&loaded_a2, true).is_err()
            || loaded_a.survives.0 != loaded_a2.survives.0,
        "{}: a2 loads to the same state as A: the comparison cannot see a leftover",
        f.name
    );
    // each area has its own salt: an area where A and a2 are equal cannot show a leftover of its content
    let differ = loaded_a.differing_areas(&loaded_a2);
    let wrong: Vec<String> = ["memory", "dsp", "settings", "state", "survives"]
        .iter()
        .filter(|area| **area != "survives" || f.survives.is_some())
        .filter_map(|area| {
            let allowed = f.salt_same.iter().any(|(a, _)| a == area);
            (differ.contains(area) == allowed).then(|| {
                std::format!(
                    "{area}: loaded A and a2 {} (salt_same {})",
                    if allowed { "differ" } else { "are equal" },
                    if allowed {
                        "lists it"
                    } else {
                        "does not list it"
                    }
                )
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{} ({}): {wrong:?}", f.name, profile(f));
    l.unload(0);
    let end_a2 = l.snapshot();
    let left = loaded_a2.survivors(
        &start,
        &end_a2,
        &|a, v| (f.kept)(&l.core, a, v),
        &|o, v| (f.kept_dsp)(&l.core, o, v),
        &|o, v| (f.rows.kept)(&l.core, o, v),
    );
    assert!(
        left.is_empty(),
        "{} ({}): the unload of a2 left what a2 wrote\n{left}",
        f.name,
        profile(f)
    );
    if let Err(d) = end_a.same_after_unload(&end_a2, true) {
        panic!(
            "{} ({}): what A leaves after its unload differs from what a2 leaves\n{d}",
            f.name,
            profile(f)
        );
    }
}

/// Case 8: a commit refused while the records are applied (`bad_late`: some rows are written, then a record is refused), then the loader's undo, leaves what the
/// unload of a module that wrote the same rows leaves (`same_but_ok`: it loads, then unloads; the rows it wrote hold the service's free values, as the undone ones
/// must). The words that turn a row off are not compared: the undo of a row that was never published sends none. The capacity counts and the service's view are those
/// of the start. The same with another module loaded, which the refusal leaves alone  (`Late::beside_b`).
fn a_refusal_during_apply_leaves_nothing<S: Service<FakeCore>>(f: &Fixture) {
    let Some(late) = &f.bad_late else {
        return;
    };
    for with_b in [false, true] {
        if with_b && !late.beside_b {
            continue;
        }
        let setup = |l: &Lifecycle<S>| {
            if with_b {
                assert_eq!(l.load(1, &f.b), OK, "{}: module B is refused", f.name);
            }
        };
        let l = Lifecycle::<S>::new(f);
        setup(&l);
        let before = l.snapshot();
        let ok = l.load(0, &late.same_but_ok);
        let at = l.at.get();
        assert_eq!(
            ok, OK,
            "{}: the reference of the late refusal is refused at {at}",
            f.name
        );
        l.unload(0);
        // (a service that parks the rows of absent modules writes MISSING for a number once its row is parked, at its tick: the refused module has no row of its own)
        if f.parks_rows {
            unsafe { S::tick(&l.core, dkrt_abi::service::tick_phase::LAST) };
        }
        let reference = l.snapshot();
        expect_small("the reference of the late refusal", f, &before, &reference);

        let l = Lifecycle::<S>::new(f);
        setup(&l);
        let r = l.load(0, &late.bad);
        assert_ne!(
            r, OK,
            "{}: the module that must be refused during apply is accepted",
            f.name
        );
        assert!(
            l.applied.get() > 0,
            "{}: the module is refused before any record was applied (error {r})",
            f.name
        );
        if f.parks_rows {
            unsafe { S::tick(&l.core, dkrt_abi::service::tick_phase::LAST) };
        }
        let refused = l.snapshot();
        expect_small("a refusal during apply", f, &before, &refused);
        // (a service that parks the rows of an absent module keeps the voice row of the module that unloaded: only the placeholder rows' bytes are left out of the compare)
        let (reference_cmp, refused_cmp) = (
            reference.without_placeholders(&f.rows),
            refused.without_placeholders(&f.rows),
        );
        if let Err(d) = reference_cmp.same_after_unload(&refused_cmp, false) {
            panic!(
                "{} ({}): the undo of a refusal during apply differs from an unload of the same rows{}\n{d}",
                f.name,
                profile(f),
                if with_b { " (beside B)" } else { "" }
            );
        }
        assert_eq!(
            l.load(0, &f.a),
            OK,
            "{}: module A is refused after the refusal",
            f.name
        );
        l.unload(0);
        if with_b {
            l.unload(1);
        }
    }
}
