//! The module list of a project and its restore (docs/design/digikit-project.md section 3, "Module list and restore"). The list is tag 1 of the `.dkp`: the content modules that were loaded at the stock save.
//!
//! - [`ModuleSet`]: a list of module ids with their store hashes, at most `MODULES_MAX` (the runtime's module limit). It is read from the loaded modules (a save records it) and from an
//!   image (a load restores it).
//! - [`ModuleRestore`]: after a project's data is installed, the listed modules that are on the card and not loaded are loaded live, one after the other, through the runtime's live job
//!   (`Env::live_load`, the same job as DIGIKIT > MODULES). It only adds: a loaded module that the list does not name stays. A refusal of the transport playing waits for the stop; a
//!   module that is not on the card, that the store cannot search, or that the runtime refuses, is one warning and one log line, and the next one goes on. A busy store or live job
//!   asks the same entry again in the next pass; a busy card does so after `CARD_WAIT_MS`, up to `CARD_TRIES_MAX` times. The store is read only while no live job stages (`Env::live_op`):
//!   the live job shares the staging buffer. A load that ended well is added to the saved selection, as the menu saves the selection of a module it loads; a path or a selection that
//!   cannot take it is the warning `NotKept`, and the module stays loaded. UI task only.
//!
//! The restore starts its loads in a tick after the one that installed the data, so the runtime's reload of the modules whose settings changed can take the live job first; a module
//! that loads later reads the project's values in its `init` either way (the overlay is in place at the install).
use super::{BoundProject, Env};
use dkrt_sdk::logk;
use dkrt_sdk::proto::E_STATE;
use dkrt_sdk::{
    E_BUSY, E_CARD_BUSY, E_DSP_NOSPACE, E_LOADED, E_NOMEM, E_NOT_FOUND, E_PLAN_FIT, E_POOL,
    E_RELOAD_PLAYING, E_SEL_PATH, E_TOO_BIG, LIVE_LOAD, OK, SEL_MAX, SEL_PATH,
};

/// The most modules of a list: the runtime's module limit.
pub const MODULES_MAX: usize = SEL_MAX;
/// The longest module id (a module record holds 31 bytes and a NUL).
pub const ID_MAX: usize = 31;
/// The store path of a module file is at most 63 bytes.
pub const MODULE_PATH_MAX: usize = 63;
/// The restore asks again after this long when the transport played.
pub const PLAY_WAIT_MS: u32 = 1000;
/// The restore asks again after this long when the card is busy, at most `CARD_TRIES_MAX` times for one entry (the way the runtime's reload of changed settings counts).
pub const CARD_WAIT_MS: u32 = 1000;
pub const CARD_TRIES_MAX: u8 = 3;

/// A module id (`<author>/<name>`, 1 to `ID_MAX` bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModuleId {
    bytes: [u8; ID_MAX],
    len: u8,
}

/// The file name of a module in the store: its id with `/` as `-`, then `.dkm` (docs/design/taxonomy.md).
pub struct ModuleFile {
    bytes: [u8; ID_MAX + 4],
    len: usize,
}

impl ModuleFile {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl ModuleId {
    pub const EMPTY: ModuleId = ModuleId {
        bytes: [0; ID_MAX],
        len: 0,
    };

    /// `None` for an id that is empty or longer than `ID_MAX`.
    pub fn new(id: &[u8]) -> Option<ModuleId> {
        if id.is_empty() || id.len() > ID_MAX {
            return None;
        }
        let mut m = ModuleId::EMPTY;
        let mut at = 0;
        while at < id.len() {
            m.bytes[at] = id[at];
            at += 1;
        }
        m.len = id.len() as u8;
        Some(m)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The module's own name: the part after the author.
    pub fn name(&self) -> &[u8] {
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            if id[at] == b'/' {
                return &id[at + 1..];
            }
            at += 1;
        }
        id
    }

    /// FNV-1a (32 bits) of the id: the `name_hash` of the SDK, the id word of `PROJECT_MODULE`.
    pub fn hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            h ^= id[at] as u32;
            h = h.wrapping_mul(0x0100_0193);
            at += 1;
        }
        h
    }

    pub fn file(&self) -> ModuleFile {
        let mut f = ModuleFile {
            bytes: [0; ID_MAX + 4],
            len: 0,
        };
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            f.bytes[at] = if id[at] == b'/' { b'-' } else { id[at] };
            at += 1;
        }
        f.bytes[at..at + 4].copy_from_slice(b".dkm");
        f.len = at + 4;
        f
    }
}

/// A store path of a module file.
#[derive(Clone, Copy)]
pub struct ModulePath {
    bytes: [u8; MODULE_PATH_MAX],
    len: u8,
}

impl ModulePath {
    pub const EMPTY: ModulePath = ModulePath {
        bytes: [0; MODULE_PATH_MAX],
        len: 0,
    };

    /// `None` for a path longer than `MODULE_PATH_MAX`.
    pub fn new(path: &[u8]) -> Option<ModulePath> {
        if path.len() > MODULE_PATH_MAX {
            return None;
        }
        let mut p = ModulePath::EMPTY;
        let mut at = 0;
        while at < path.len() {
            p.bytes[at] = path[at];
            at += 1;
        }
        p.len = path.len() as u8;
        Some(p)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Can an entry of the saved selection hold this path (`SEL_PATH` bytes with the NUL)? The live load takes paths up to `MODULE_PATH_MAX`.
    pub fn fits_selection(&self) -> bool {
        self.len > 0 && (self.len as usize) < SEL_PATH
    }
}

/// A content module that is loaded: its id and the store path it was loaded from.
#[derive(Clone, Copy)]
pub struct LoadedModule {
    pub id: ModuleId,
    pub path: ModulePath,
}

/// The state of the runtime's live job as the restore reads it (`dkrt_live_state`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveJob {
    /// 0 no job yet, 1 queued or running, 2 ended.
    pub phase: u32,
    /// The refusal or failure of an ended job (0: none).
    pub result: i32,
    /// The job's operation (`LIVE_LOAD`, ...).
    pub op: u32,
}

/// Why a module of the list is not loaded: the warning of `Env::warn_module`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleWarning {
    /// The module's file is not on the card.
    Missing,
    /// The runtime refused the load because the module does not fit (memory, pool, plan or DSP space).
    NoFit,
    /// The runtime refused or failed the load for another reason (not compatible, read error, ...), or the store could not be searched.
    Refused,
    /// The module is loaded but not saved in the selection: it is not loaded again at the next boot.
    NotKept,
}

/// A list of modules with the store hash of each file, in order.
#[derive(Clone, Copy)]
pub struct ModuleSet {
    ids: [ModuleId; MODULES_MAX],
    hashes: [u32; MODULES_MAX],
    count: usize,
}

impl ModuleSet {
    pub const fn new() -> ModuleSet {
        ModuleSet {
            ids: [ModuleId::EMPTY; MODULES_MAX],
            hashes: [0; MODULES_MAX],
            count: 0,
        }
    }

    /// The content modules that are loaded now, in the order of the runtime's module records, each with the store hash of its file.
    pub fn of_loaded<E: Env + ?Sized>(env: &E) -> ModuleSet {
        let mut set = ModuleSet::new();
        let mut record = 0;
        while record < MODULES_MAX {
            if let Some(module) = env.loaded_module(record) {
                set.push(module.id, env.store_hash(module.path.as_bytes()));
            }
            record += 1;
        }
        set
    }

    /// The module list of an image (an id that is too long for a module is left out).
    pub fn of_image(image: &[u8]) -> ModuleSet {
        let mut set = ModuleSet::new();
        for (id, hash) in crate::dkp::modules(image) {
            if let Some(id) = ModuleId::new(id) {
                set.push(id, hash);
            }
        }
        set
    }

    fn push(&mut self, id: ModuleId, hash: u32) {
        if self.count < MODULES_MAX {
            self.ids[self.count] = id;
            self.hashes[self.count] = hash;
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn id(&self, at: usize) -> &ModuleId {
        &self.ids[at]
    }

    pub fn has(&self, id: &ModuleId) -> bool {
        let mut at = 0;
        while at < self.count {
            if self.ids[at] == *id {
                return true;
            }
            at += 1;
        }
        false
    }

    /// Both lists hold the same ids with the same hashes in the same order.
    pub fn same_as(&self, other: &ModuleSet) -> bool {
        if self.count != other.count {
            return false;
        }
        let mut at = 0;
        while at < self.count {
            if self.ids[at] != other.ids[at] || self.hashes[at] != other.hashes[at] {
                return false;
            }
            at += 1;
        }
        true
    }

    /// The list as `dkp::set_modules` takes it: `out[..len]` is filled, the number is returned.
    pub fn list<'a>(&'a self, out: &mut [(&'a [u8], u32); MODULES_MAX]) -> usize {
        let mut at = 0;
        while at < self.count {
            out[at] = (self.ids[at].as_bytes(), self.hashes[at]);
            at += 1;
        }
        self.count
    }
}

impl Default for ModuleSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the restore is with the entry at `ModuleRestore::at`.
#[repr(u8)] // an explicit tag, so that `Next` is all zero (the service is a bss static)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// No job is under way: the next tick looks for the entry's file.
    Next,
    /// The file of the entry is found (`ModuleRestore::file`): the next tick queues the live load.
    Queue,
    /// The live job of the entry is queued or running.
    Running,
    /// The transport played or the card was busy: the load of the entry is asked again from this time (ms, wraps).
    Waiting { until: u32 },
}

/// The restore of one project's module list. Owns the pending list, the entry it is at, the job in flight and the wait for the transport.
pub struct ModuleRestore {
    /// The modules still to look at, from `at` on.
    pending: ModuleSet,
    at: usize,
    step: Step,
    /// The store path of the entry's file, from the step `Queue` on.
    file: ModulePath,
    /// The busy-card refusals of the entry.
    card_tries: u8,
    /// The project change the list came from (`BoundProject::epoch`), and the slot of the project, for the log.
    epoch: u32,
    slot: u32,
}

impl ModuleRestore {
    pub const fn new() -> ModuleRestore {
        ModuleRestore {
            pending: ModuleSet::new(),
            at: 0,
            step: Step::Next,
            file: ModulePath::EMPTY,
            card_tries: 0,
            epoch: 0,
            slot: 0,
        }
    }

    /// Is a module of the list still to be looked at, or its job running? For a host test that drives the service by hand.
    pub fn busy(&self) -> bool {
        self.at < self.pending.len()
    }

    /// One UI pass. A new project (the bound project's data was installed since the last pass) replaces the pending list with its own and starts nothing in this pass.
    pub fn tick<E: Env + ?Sized>(&mut self, env: &mut E, bound: &BoundProject) {
        if bound.epoch() != self.epoch {
            self.replace(bound);
            return;
        }
        if !self.busy() {
            return;
        }
        if let Step::Waiting { until } = self.step {
            if (env.now_ms().wrapping_sub(until) as i32) < 0 {
                return;
            }
            self.step = Step::Queue;
        }
        match self.step {
            Step::Running => self.poll_job(env),
            Step::Queue => self.queue_entry(env),
            _ => self.begin_entry(env),
        }
    }

    /// The pending list becomes the module list of the project that is bound now: none for no project or a stock project. A job of the old list that runs goes on in the runtime;
    /// the next load waits for it (`E_BUSY`).
    fn replace(&mut self, bound: &BoundProject) {
        self.epoch = bound.epoch();
        self.slot = bound.slot().as_i32() as u32;
        self.pending = ModuleSet::of_image(bound.image_bytes());
        self.at = 0;
        self.step = Step::Next;
    }

    fn entry(&self) -> ModuleId {
        *self.pending.id(self.at)
    }

    /// The entry is done (loaded, skipped or refused): the next one.
    fn advance(&mut self) {
        self.at += 1;
        self.card_tries = 0;
        self.step = Step::Next;
    }

    /// One `PROJECT_MODULE` event for the entry.
    fn log<E: Env + ?Sized>(&self, env: &mut E, what: u32, status: i32) {
        env.log(
            logk::PROJECT_MODULE,
            [
                self.slot,
                what,
                status as u32,
                self.at as u32,
                self.entry().hash(),
            ],
        );
    }

    /// Is a module with this id loaded? The ids only: no store read.
    fn is_loaded<E: Env + ?Sized>(env: &E, id: &ModuleId) -> bool {
        let mut record = 0;
        while record < MODULES_MAX {
            if let Some(module) = env.loaded_module(record) {
                if module.id == *id {
                    return true;
                }
            }
            record += 1;
        }
        false
    }

    /// Look at the entry: skip a module that is loaded, find the file of one that is not (the one search of the entry) and go on to queue its live load; warn for one that is not on the
    /// card or cannot be searched for. The search reads the store, so it waits while a live job stages.
    fn begin_entry<E: Env + ?Sized>(&mut self, env: &mut E) {
        let id = self.entry();
        if Self::is_loaded(env, &id) {
            self.advance();
            return;
        }
        if env.live_op() != 0 {
            return;
        }
        match env.find_module_file(id.file().as_bytes()) {
            Ok(path) => {
                self.file = path;
                self.step = Step::Queue;
            }
            // the store is held by another: the same entry in the next pass
            Err(E_BUSY) => {}
            Err(E_NOT_FOUND) => {
                self.log(env, Event::MISSING, 0);
                env.warn_module(ModuleWarning::Missing, &id);
                self.advance();
            }
            // the store could not be searched: the module is not known to be missing
            Err(status) => {
                self.log(env, Event::REFUSED, status);
                env.warn_module(ModuleWarning::Refused, &id);
                self.advance();
            }
        }
    }

    /// Queue the live load of the entry's file. A path the saved selection cannot take is told when the load is queued; the load goes on.
    fn queue_entry<E: Env + ?Sized>(&mut self, env: &mut E) {
        let id = self.entry();
        let file = self.file;
        let queued = env.live_load(&file);
        if queued == OK {
            self.step = Step::Running;
            self.log(env, Event::QUEUED, 0);
            if !file.fits_selection() {
                self.not_kept(env, E_SEL_PATH, &id);
            }
        } else {
            self.not_started(env, queued, &id);
        }
    }

    /// The module is loaded and will not be loaded at the next boot: one log line and one warning.
    fn not_kept<E: Env + ?Sized>(&self, env: &mut E, status: i32, id: &ModuleId) {
        self.log(env, Event::NOT_KEPT, status);
        env.warn_module(ModuleWarning::NotKept, id);
    }

    /// The runtime did not queue the job, or the job ended with a refusal, with `status`.
    fn not_started<E: Env + ?Sized>(&mut self, env: &mut E, status: i32, id: &ModuleId) {
        if status == E_BUSY || status == E_STATE {
            // another live job or a store write: the same load again in the next pass
            self.step = Step::Queue;
        } else if status == E_CARD_BUSY && self.card_tries < CARD_TRIES_MAX {
            self.card_tries += 1;
            self.step = Step::Waiting {
                until: env.now_ms().wrapping_add(CARD_WAIT_MS),
            };
        } else if status == E_RELOAD_PLAYING {
            // load on stop: asked again every `PLAY_WAIT_MS` until the transport stops, and counted for no try
            self.step = Step::Waiting {
                until: env.now_ms().wrapping_add(PLAY_WAIT_MS),
            };
        } else if status == E_LOADED {
            self.advance();
        } else {
            self.log(env, Event::REFUSED, status);
            let fit = [E_NOMEM, E_TOO_BIG, E_POOL, E_PLAN_FIT, E_DSP_NOSPACE].contains(&status);
            env.warn_module(
                if fit {
                    ModuleWarning::NoFit
                } else {
                    ModuleWarning::Refused
                },
                id,
            );
            self.advance();
        }
    }

    /// The job of the entry runs: when it ended, take its result. The job state is the runtime's, shared with the menu and the reload of changed settings; a job that is not a load
    /// is another one's, and the entry is looked at again.
    fn poll_job<E: Env + ?Sized>(&mut self, env: &mut E) {
        let job = env.live_job();
        if job.phase == 1 {
            return;
        }
        let id = self.entry();
        if Self::is_loaded(env, &id) {
            // a path that cannot be kept was told when the load was queued
            let selection = if self.file.fits_selection() {
                env.selection_add(&self.file)
            } else {
                OK
            };
            self.log(env, Event::DONE, selection);
            if selection != OK {
                self.not_kept(env, selection, &id);
            }
            self.advance();
        } else if job.op == LIVE_LOAD && job.phase == 2 && job.result != OK {
            self.not_started(env, job.result, &id);
        } else {
            self.step = Step::Queue;
        }
    }
}

impl Default for ModuleRestore {
    fn default() -> Self {
        Self::new()
    }
}

/// The `what` word of `PROJECT_MODULE`.
pub struct Event;

impl Event {
    /// The live load was queued.
    pub const QUEUED: u32 = 1;
    /// The load ended well; the status is the result of adding the module to the saved selection.
    pub const DONE: u32 = 2;
    /// The module is not on the card.
    pub const MISSING: u32 = 3;
    /// The runtime refused the load, or it failed; the status is the refusal.
    pub const REFUSED: u32 = 4;
    /// The module is loaded but cannot be kept in the saved selection; the status is why.
    pub const NOT_KEPT: u32 = 5;
}
