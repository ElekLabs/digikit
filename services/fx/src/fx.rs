//! The owner object of the `fx` service. It holds the collection of one module's builder calls (`Builder`) and what the effect guard has logged (`FxGuard`); the effect pages, the
//! send cells and the effect rows are tables the service keeps by name (`fxpage::DKRT_FXP*`, `fxsend::DKRT_FXS`, `rows::DKRT_FROWS`: see README.md for the reader of each).
//! `Provider` (`start`, `detach`), `Ticker`, `Reentry` and `Describe` are here, in one block each; `records.rs`, `dsp/part.rs` and `queries.rs` hold the other parts.
use crate::builder::Builder;
use crate::dsp::FxGuard;
use crate::fxpage;
use crate::fxsend;
use crate::routing::{self, Routing};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{ConsumerId, Describe, Provider, Reentry, Status, Ticker, WordSink};
use dkrt_abi::service::{tick_phase, TableFault};
use dkrt_sdk::proto::{CAPK_FX_SLOTS, CAPK_SEND_SLOTS};

/// The fx service: nothing collected, nothing logged. The file's cell holds the one instance (`Provider::start`).
pub struct Fx {
    builder: Builder,
    guard: FxGuard,
    routing: Routing,
    /// The UI handle the FX ROUTING row was added with (0: the image has no UI, or the row was refused).
    ui: usize,
}

impl Fx {
    /// An owner with nothing collected and nothing logged, and no table bound (`start` binds it).
    pub const fn new() -> Fx {
        Fx {
            builder: Builder::new(),
            guard: FxGuard::new(),
            routing: Routing::new(),
            ui: 0,
        }
    }

    /// The collection of the module whose `init` is open: the builder calls of the service file reach it here.
    pub fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    /// What the FX ROUTING steps keep between two popups, and the UI handle they show them with (0: none).
    pub fn routing(&mut self) -> (&mut Routing, usize) {
        (&mut self.routing, self.ui)
    }

    /// What the guard's diagnostics have logged already.
    pub(crate) fn guard(&mut self) -> &mut FxGuard {
        &mut self.guard
    }
}

impl Default for Fx {
    fn default() -> Fx {
        Fx::new()
    }
}

impl<C: CoreApi> Provider<C> for Fx {
    /// The file's `init`, before its hooks are installed: the table of the effect pages is bound (with the firmware words of the hooks), or the file is refused (`E_NO_REGISTRY`:
    /// no usable `core/fxpages` in the image).
    fn start(core: &C) -> Result<Fx, Status> {
        // SAFETY: the table is this crate's static; the words are the file's own data (none on a host)
        let r = unsafe { fxpage::bind(core, fxpage::firmware()) };
        if r != dkrt_sdk::OK {
            return Err(r);
        }
        let mut fx = Fx::new();
        fx.ui = routing::attach();
        Ok(fx)
    }

    // An effect module is torn down like any other (`can_stop` keeps its default): the loader's `DspPart::clear` (the row's pc to "free" first, so the chain skips it at once: the
    // master chain passes the signal dry, a send bus returns nothing) and `detach` (page, list, send cell, defaults) are the teardown, whatever route the effect runs on. The route and
    // the send amounts are settings keyed by the module id, so the next load of the module gets them back; the knob values stay in the kit's array (`fxpage::apply`).
    fn detach(&mut self, core: &C, consumer: ConsumerId) {
        self.builder.end();
        // the wait of a reload that tore this effect down some other way is over
        self.routing.effect_gone(core, consumer.get());
        // SAFETY: the pages and send cells are this crate's tables; `consumer` is a module record + 1
        unsafe { fxpage::remove(core, consumer.get()) };
    }
}

impl<C: CoreApi> Ticker<C> for Fx {
    fn tick(&mut self, core: &C, phase: u32) {
        if phase == tick_phase::LATE {
            // SAFETY: the UI task runs the tick; the pages and the send cells are this crate's tables
            unsafe { fxpage::tick(core) };
            // the end of the reload a pick queued
            routing::poll(&mut self.routing, core, self.ui);
        }
    }
}

impl<C: CoreApi> Reentry<C> for Fx {
    type Event = routing::Event;
    /// A pick and a list asked for while a call of the service runs: the UI task makes one at a time, so a few wait at most.
    const QUEUE: usize = 4;

    /// A UI callback's step, run with the owner free (a pick that arrived inside another call of the service waited for it).
    fn apply(&mut self, core: &C, event: routing::Event) {
        routing::apply(&mut self.routing, core, self.ui, event);
    }
}

impl<C: CoreApi> Describe<C> for Fx {
    fn cap(&self, core: &C, kind: u8) -> Option<(u32, u32)> {
        // SAFETY: reads the image's tables through the core and the rows this crate owns
        unsafe {
            if kind == CAPK_FX_SLOTS {
                Some((crate::dsp::fx_free_rows(core), crate::dsp::fx_used(core)))
            } else if kind == CAPK_SEND_SLOTS {
                Some((fxsend::slots(core), fxsend::used()))
            } else {
                None
            }
        }
    }

    fn table_check(&self, _core: &C, used: u32, out: &mut TableFault) -> u32 {
        // SAFETY: reads the rows this crate owns
        unsafe { crate::dsp::table_check(used, out) }
    }

    /// One word for each effect row of module record `who` - 1 that has a page (`fxsend::describe`); none for a module with no such row.
    fn describe(core: &C, who: ConsumerId, out: &mut WordSink) {
        fxsend::describe(core, who.get(), out)
    }
}
