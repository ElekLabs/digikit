//! FX ROUTING: the SETTINGS list of the effects with a page, the popup of the routes one may take, and what a pick does (docs/findings/64-fx-send-page.md for the send routes).
//!
//! The logic runs over `CoreApi` and the small trait `Ui`; the service file implements `Ui` with the UI SDK (`dks/src/ui.rs`) and calls the three steps from its callbacks:
//!
//! - `list` (the SETTINGS row): one row for each effect with a page, `NAME: SEND` or `NAME: MASTER`;
//! - `pick_effect` (a row of the list): the routes the effect may take now (`fxsend::effect`);
//! - `pick_route` (a route): on a change, save `fx/route/<module id>` first, then queue the reload of the effect's module; the reload registers the effect again and reads the key.
//!
//! A callback of the UI reaches the steps as an `Event` through the owner's `Reentry` queue (`Routing::run`), so a pick that arrives while a call of the service runs is queued, never lost.
//!
//! The steps keep what the next step needs (the rows listed, the effect chosen) in `Routing`, a field of the owner.
use crate::fxsend::{self, Effect, Route};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_sdk::{E_BUSY, E_RELOAD_PLAYING, LIVE_RELOAD, OK};

/// The popup a step shows next: its `done` callback calls the matching step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Effect,
    Route,
}

/// What a UI callback asks of the steps; the owner's `Reentry` queue carries it to `Routing::run`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The SETTINGS row was opened.
    List,
    /// Row `n` of the effect list was chosen.
    Effect(u8),
    /// Route `n` of the route popup was chosen.
    Route(u8),
}

/// The UI the steps use (a choice popup and a toast). A `Line` is NUL terminated and cut to what the popup shows, so the adapter takes its bytes as they are.
pub trait Ui {
    /// Show a choice of `items`; `step` is the step its answer runs. False when it could not be shown.
    fn choice(&mut self, title: &Line, items: &[Line], step: Step) -> bool;
    fn toast(&mut self, text: &Line);
}

/// Rows the list can have: one for each effect row of the image.
pub const LIST_MAX: usize = MAX_FX;
/// Characters of an effect's name in the list and in toasts.
pub const NAME_MAX: usize = 12;
/// Bytes of a line, the NUL included (`DK_UI_ROW` and `DK_UI_TOAST` of <dk/platform/ui.h> are not longer).
pub const LINE_MAX: usize = 32;

/// What follows the route in the toast of a reload that waits for the transport to stop, and of one that is queued or running.
const ON_STOP: &[u8] = b" ON STOP";
const PENDING: &[u8] = b" PENDING";

/// The route texts, indexed by `Route::byte` (0 master, 1 send): the one copy of each.
const ROUTE_TEXT: [&[u8]; 2] = [b"MASTER", b"SEND"];

/// A line of text on the stack: upper case ASCII, NUL terminated, cut to its size.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Line {
    buf: [u8; LINE_MAX],
    len: usize,
}

impl Line {
    pub fn new() -> Line {
        Line {
            buf: [0; LINE_MAX],
            len: 0,
        }
    }

    /// A line of `text`.
    pub fn of(text: &str) -> Line {
        let mut line = Line::new();
        line.put(text.as_bytes());
        line
    }

    /// Add `text`; a byte that is not ASCII shows as `?`, so the line always is valid text.
    pub fn put(&mut self, text: &[u8]) -> &mut Line {
        for b in text {
            if self.len < LINE_MAX - 1 {
                self.buf[self.len] = if b.is_ascii() {
                    b.to_ascii_uppercase()
                } else {
                    b'?'
                };
                self.len += 1;
            }
        }
        self
    }

    /// The bytes with their terminator, for the C side.
    pub fn as_ptr(&self) -> *const u8 {
        self.buf.as_ptr()
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: `put` stores ASCII only
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }
}

impl Default for Line {
    fn default() -> Line {
        Line::new()
    }
}

/// The name of a module id for the list: the part after the last `/`, cut to `NAME_MAX`.
fn name_of(id: &[u8]) -> &[u8] {
    let name = id.rsplit(|c| *c == b'/').next().unwrap_or(id);
    &name[..name.len().min(NAME_MAX)]
}

fn route_text(route: Route) -> &'static [u8] {
    ROUTE_TEXT[route.byte() as usize]
}

/// What the steps keep between two popups.
pub struct Routing {
    /// The effect rows of the list, with the owner each had when it was shown.
    rows: [(u8, u8); MAX_FX],
    listed: usize,
    /// The effect row the second popup is for, and the routes it offers (send first).
    row: usize,
    offered: [Route; 2],
    offered_len: usize,
    /// The reload of the last pick is queued: the module id, its route and whether the toast of its end is due.
    waiting: Option<Waiting>,
}

/// The pick whose reload the service waits for: the job it queued (by serial), or, once that job refused for the transport, the reload the runtime does at stop.
#[derive(Clone, Copy)]
struct Waiting {
    id: [u8; 32],
    id_len: usize,
    /// The module record + 1 the effect had when the pick was made.
    owner: u8,
    route: Route,
    /// The serial of the live job the pick queued (`DkrtLive::serial`).
    serial: u32,
    /// The job was refused for the playing transport: the runtime reloads the module at stop.
    deferred: bool,
}

impl Waiting {
    fn id(&self) -> &[u8] {
        &self.id[..self.id_len]
    }
}

impl Routing {
    pub const fn new() -> Routing {
        Routing {
            rows: [(0, 0); MAX_FX],
            listed: 0,
            row: 0,
            offered: [Route::Send; 2],
            offered_len: 0,
            waiting: None,
        }
    }

    /// Run the step a UI callback asked for.
    pub fn run<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U, event: Event) {
        match event {
            Event::List => self.list(core, ui),
            Event::Effect(i) => self.pick_effect(core, ui, i as usize),
            Event::Route(i) => self.pick_route(core, ui, i as usize),
        }
    }

    /// The SETTINGS row was opened: the list of the effects. A toast when no effect has a page.
    pub fn list<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U) {
        let mut lines = [Line::new(); MAX_FX];
        self.listed = 0;
        // SAFETY: the UI task runs the callbacks of the service
        let sends = unsafe { fxsend::saved_sends(core) };
        for k in 0..MAX_FX {
            // SAFETY: as above
            let Some(effect) = (unsafe { fxsend::effect_in(core, k, sends) }) else {
                continue;
            };
            lines[self.listed]
                .put(name_of(effect.id()))
                .put(b": ")
                .put(route_text(effect.saved));
            self.rows[self.listed] = (k as u8, effect.owner);
            self.listed += 1;
        }
        if self.listed == 0 {
            ui.toast(&Line::of("NO EFFECTS LOADED"));
            return;
        }
        ui.choice(&Line::of("FX ROUTING"), &lines[..self.listed], Step::Effect);
    }

    /// Row `index` of the list was chosen: the routes the effect may take now.
    pub fn pick_effect<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U, index: usize) {
        let Some((k, effect)) = self.chosen(core, index) else {
            ui.toast(&Line::of("NO EFFECTS LOADED"));
            return;
        };
        self.row = k;
        self.offered_len = 0;
        if effect.send_allowed {
            self.offered[self.offered_len] = Route::Send;
            self.offered_len += 1;
        }
        if effect.master_allowed {
            self.offered[self.offered_len] = Route::Master;
            self.offered_len += 1;
        }
        let name = name_of(effect.id());
        if self.offered_len == 0 {
            ui.toast(Line::new().put(name).put(b": NO ROUTE FREE"));
            return;
        }
        let mut items = [Line::new(); 2];
        for (item, route) in items.iter_mut().zip(&self.offered[..self.offered_len]) {
            item.put(route_text(*route));
        }
        ui.choice(
            Line::new().put(name).put(b" ROUTE"),
            &items[..self.offered_len],
            Step::Route,
        );
    }

    /// Route `index` of the second popup was chosen. A change saves the route first (a failed save queues nothing), then queues the reload of the effect's module; the job runs
    /// later from the UI loop and reads the key. A busy job refuses the reload: the old route is written back, so the same pick can be made again. The toast of a queued
    /// reload comes from `poll_job`, when the job has ended.
    pub fn pick_route<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U, index: usize) {
        if index >= self.offered_len {
            return;
        }
        let route = self.offered[index];
        // SAFETY: the UI task runs the callbacks of the service
        let Some(effect) = (unsafe { fxsend::effect(core, self.row) }) else {
            ui.toast(&Line::of("NO EFFECTS LOADED"));
            return;
        };
        let mut line = Line::new();
        line.put(name_of(effect.id())).put(b": ");
        // a deferred wait may be stale (the runtime gave up its retry at stop): a new pick runs the whole pick again and replaces it
        let stale = matches!(self.waiting, Some(w) if w.deferred && w.owner == effect.owner && w.id() == effect.id());
        if route != effect.saved || stale {
            let changed = route != effect.saved;
            // SAFETY: as above
            if changed && unsafe { fxsend::save_route(core, self.row, route) } != 0 {
                ui.toast(line.put(b"NOT SAVED"));
                return;
            }
            // SAFETY: as above; the path is NUL terminated and lives in the runtime's module table
            let queued = unsafe { core.live(LIVE_RELOAD, core.module_path(effect.owner)) };
            if queued == E_BUSY {
                if changed {
                    // SAFETY: as above; the old value was readable, so this put is the one that could fail only the same way
                    unsafe { fxsend::save_route(core, self.row, effect.saved) };
                }
                ui.toast(&Line::of("BUSY: TRY AGAIN"));
                return;
            }
            if queued != OK {
                ui.toast(line.put(b"RESTART TO APPLY"));
                return;
            }
            // the job may still refuse (the transport plays, a menu is open): its end decides the toast (`poll_job`)
            // SAFETY: as above; the job just queued is the one the state describes
            let serial = unsafe { core.live_state() }.serial;
            self.waiting = Some(Waiting {
                id: effect.id,
                id_len: effect.id_len,
                owner: effect.owner,
                route,
                serial,
                deferred: false,
            });
            return;
        }
        // the saved route is not the applied one while its reload waits
        match self.waiting {
            Some(w) if w.owner == effect.owner && w.id() == effect.id() => {
                line.put(route_text(w.route))
                    .put(if w.deferred { ON_STOP } else { PENDING });
            }
            _ => {
                line.put(route_text(route));
            }
        }
        ui.toast(&line);
    }

    /// Once per UI pass: the end of the reload a pick queued, found by the job's serial (the live state holds the last job only: a job that started after ours leaves our end in
    /// `done_*`). Success is the route's toast; a transport that plays defers the reload to its stop (`reload_when_possible`: the runtime retries until then) and says so; any other
    /// refusal leaves the saved route for the next boot.
    pub fn poll_job<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U) {
        let Some(w) = self.waiting else { return };
        if w.deferred {
            return;
        }
        // SAFETY: the UI task runs the tick
        let job = unsafe { core.live_state() };
        let result = if job.serial == w.serial {
            if job.phase != 2 {
                return;
            }
            job.result
        } else if job.done_serial == w.serial {
            job.done_result
        } else {
            // the job's end is gone (the state moved on by more than one job): nothing to say
            self.waiting = None;
            return;
        };
        let id = w.id();
        let mut line = Line::new();
        line.put(name_of(id)).put(b": ");
        if result == OK {
            self.waiting = None;
            line.put(route_text(w.route));
        } else if result == E_RELOAD_PLAYING {
            // SAFETY: as above
            unsafe { core.reload_when_possible(id) };
            self.waiting = Some(Waiting {
                deferred: true,
                ..w
            });
            line.put(route_text(w.route)).put(ON_STOP);
        } else {
            self.waiting = None;
            line.put(b"RESTART TO APPLY");
        }
        ui.toast(&line);
    }

    /// The effect of module record `owner` (+ 1) is torn down (`Provider::detach`). The teardown of the reload this service queued is not its end: the job is running and the wait
    /// stays. Any other teardown (an unload, another reload, the deferred reload at stop) answers the wait: nothing is due.
    pub fn effect_gone<C: CoreApi>(&mut self, core: &C, owner: u8) {
        let Some(w) = self.waiting else { return };
        if w.owner != owner {
            return;
        }
        // SAFETY: the UI task runs `detach`
        let job = unsafe { core.live_state() };
        if !w.deferred && job.phase == 1 && job.serial == w.serial {
            return;
        }
        self.waiting = None;
    }

    /// The effect of list row `index`, when it still is the one that was listed.
    fn chosen<C: CoreApi>(&self, core: &C, index: usize) -> Option<(usize, Effect)> {
        let (k, owner) = *self.rows[..self.listed].get(index)?;
        // SAFETY: the UI task runs the callbacks of the service
        let effect = unsafe { fxsend::effect(core, k as usize) }?;
        (effect.owner == owner).then_some((k as usize, effect))
    }
}

/// The UI of core/ui for the owner's `Reentry::apply` and tick: the functions of the service file (`dks/src/ui.rs`), linked with this crate. A host build has none.
#[cfg(target_arch = "m68k")]
mod handle {
    use super::{Line, Step, Ui};
    extern "C" {
        fn dk_fx_ui_choice(
            ui: usize,
            title: *const Line,
            items: *const Line,
            n: usize,
            step: u32,
        ) -> bool;
        fn dk_fx_ui_toast(ui: usize, text: *const Line);
    }
    pub struct HandleUi(pub usize);
    impl Ui for HandleUi {
        fn choice(&mut self, title: &Line, items: &[Line], step: Step) -> bool {
            // SAFETY: the pointers are live for the call
            unsafe { dk_fx_ui_choice(self.0, title, items.as_ptr(), items.len(), step as u32) }
        }
        fn toast(&mut self, text: &Line) {
            // SAFETY: as above
            unsafe { dk_fx_ui_toast(self.0, text) }
        }
    }
}
#[cfg(target_arch = "m68k")]
use handle::HandleUi;

/// Run `event` with the UI of `handle` (the `ui` of the owner): nothing for no UI.
pub(crate) fn apply<C: CoreApi>(routing: &mut Routing, core: &C, handle: usize, event: Event) {
    #[cfg(target_arch = "m68k")]
    {
        if handle != 0 {
            routing.run(core, &mut HandleUi(handle), event);
        }
    }
    #[cfg(not(target_arch = "m68k"))]
    {
        let _ = (routing, core, handle, event);
    }
}

/// `Routing::poll_job` with the UI of `handle`; nothing for no UI.
pub(crate) fn poll<C: CoreApi>(routing: &mut Routing, core: &C, handle: usize) {
    #[cfg(target_arch = "m68k")]
    {
        if handle != 0 {
            routing.poll_job(core, &mut HandleUi(handle));
        }
    }
    #[cfg(not(target_arch = "m68k"))]
    {
        let _ = (routing, core, handle);
    }
}

/// The UI handle for the FX ROUTING row, from the service file (`dks/src/ui.rs`: `dk_ui_open` and `dk_ui_settings_row`, both optional imports): 0 when the image has no UI or refuses the
/// row. A host build has no file, so no row.
pub(crate) fn attach() -> usize {
    #[cfg(target_arch = "m68k")]
    {
        extern "C" {
            fn dk_fx_routing_attach() -> usize;
        }
        // SAFETY: a function of the service file, linked with this crate; `init` runs it in the UI task
        unsafe { dk_fx_routing_attach() }
    }
    #[cfg(not(target_arch = "m68k"))]
    {
        0
    }
}

impl Default for Routing {
    fn default() -> Routing {
        Routing::new()
    }
}

#[cfg(test)]
mod tests;
