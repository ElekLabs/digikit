//! The restore of one project's module list.
use crate::engine::restore::*;
use crate::engine::{BoundProject, Env};
use dkrt_sdk::logk;
use dkrt_sdk::proto::E_STATE;
use dkrt_sdk::{
    E_BUSY, E_CARD_BUSY, E_DSP_NOSPACE, E_LOADED, E_NOMEM, E_NOT_FOUND, E_PLAN_FIT, E_POOL,
    E_RELOAD_PLAYING, E_SEL_PATH, E_TOO_BIG, LIVE_LOAD, OK,
};

/// The restore asks again after this long when the transport played.
pub const PLAY_WAIT_MS: u32 = 1000;
/// The restore asks again after this long when the card is busy, at most `CARD_TRIES_MAX` times for one entry (the way the runtime's reload of changed settings counts).
pub const CARD_WAIT_MS: u32 = 1000;
pub const CARD_TRIES_MAX: u8 = 3;

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
