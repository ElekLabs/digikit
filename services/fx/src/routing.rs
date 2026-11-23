//! FX ROUTING: the SETTINGS list of the effects with a page, the popup of the routes one may take, and what a pick does (docs/findings/64-fx-send-page.md for the send routes).
//!
//! The logic runs over `CoreApi` and the small trait `Ui`; the service file implements `Ui` with the UI SDK (`dks/src/ui.rs`) and calls the three steps from its callbacks:
//!
//! - `list` (the SETTINGS row): one row for each effect with a page, `NAME: SEND` or `NAME: MASTER`;
//! - `pick_effect` (a row of the list): the routes the effect may take now (`fxsend::effect`);
//! - `pick_route` (a route): on a change, queue the reload of the effect's module and save `fx/route/<module id>`; the reload registers the effect again and reads the key.
//!
//! The steps keep what the next step needs (the rows listed, the effect chosen) in `Routing`, a field of the owner.
use crate::fxsend::{self, Effect, Route};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_sdk::{E_BUSY, LIVE_RELOAD};

/// The popup a step shows next: its `done` callback calls the matching step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Effect,
    Route,
}

/// The UI the steps use (a choice popup and a toast); the text has no terminator and is already cut to what the popup shows.
pub trait Ui {
    /// Show a choice of `items`; `step` is the step its answer runs. False when it could not be shown.
    fn choice(&mut self, title: &str, items: &[&str], step: Step) -> bool;
    fn toast(&mut self, text: &str);
}

/// Rows the list can have: one for each effect row of the image.
pub const LIST_MAX: usize = MAX_FX;
/// Characters of an effect's name in the list and in toasts.
pub const NAME_MAX: usize = 12;
/// The longest line: a name, `: MASTER`.
const LINE_MAX: usize = 32;

/// A line of text on the stack: upper case, cut to its size.
struct Line {
    buf: [u8; LINE_MAX],
    len: usize,
}

impl Line {
    fn new() -> Line {
        Line {
            buf: [0; LINE_MAX],
            len: 0,
        }
    }

    fn put(&mut self, text: &[u8]) -> &mut Line {
        for b in text {
            if self.len < LINE_MAX {
                self.buf[self.len] = b.to_ascii_uppercase();
                self.len += 1;
            }
        }
        self
    }

    fn str(&self) -> &str {
        // only ASCII bytes of module ids and literals go in; anything else shows as a failure of this line, never a panic
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

/// The name of a module id for the list: the part after the last `/`, cut to `NAME_MAX`.
fn name_of(id: &[u8]) -> &[u8] {
    let name = id.rsplit(|c| *c == b'/').next().unwrap_or(id);
    &name[..name.len().min(NAME_MAX)]
}

fn route_text(route: Route) -> &'static [u8] {
    match route {
        Route::Send => b"SEND",
        Route::Master => b"MASTER",
    }
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
}

impl Routing {
    pub const fn new() -> Routing {
        Routing {
            rows: [(0, 0); MAX_FX],
            listed: 0,
            row: 0,
            offered: [Route::Send; 2],
            offered_len: 0,
        }
    }

    /// The SETTINGS row was opened: the list of the effects. A toast when no effect has a page.
    pub fn list<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U) {
        let mut lines: [Line; MAX_FX] = core::array::from_fn(|_| Line::new());
        self.listed = 0;
        for k in 0..MAX_FX {
            // SAFETY: the UI task runs the callbacks of the service
            let Some(effect) = (unsafe { fxsend::effect(core, k) }) else {
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
            ui.toast("NO EFFECTS LOADED");
            return;
        }
        let items: [&str; MAX_FX] = core::array::from_fn(|i| lines[i].str());
        ui.choice("FX ROUTING", &items[..self.listed], Step::Effect);
    }

    /// Row `index` of the list was chosen: the routes the effect may take now.
    pub fn pick_effect<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U, index: usize) {
        let Some((k, effect)) = self.chosen(core, index) else {
            ui.toast("NO EFFECTS LOADED");
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
            ui.toast(Line::new().put(name).put(b": NO ROUTE FREE").str());
            return;
        }
        let mut items = [""; 2];
        for (item, route) in items.iter_mut().zip(&self.offered[..self.offered_len]) {
            *item = match route {
                Route::Send => "SEND",
                Route::Master => "MASTER",
            };
        }
        ui.choice(
            Line::new().put(name).put(b" ROUTE").str(),
            &items[..self.offered_len],
            Step::Route,
        );
    }

    /// Route `index` of the second popup was chosen. A change queues the reload of the effect's module first (a busy job refuses it, and then nothing is saved, so the same pick
    /// can be made again), then saves the route; the job runs later from the UI loop and reads it. A reload the runtime refuses leaves the saved route for the next boot.
    pub fn pick_route<C: CoreApi, U: Ui>(&mut self, core: &C, ui: &mut U, index: usize) {
        if index >= self.offered_len {
            return;
        }
        let route = self.offered[index];
        // SAFETY: the UI task runs the callbacks of the service
        let Some(effect) = (unsafe { fxsend::effect(core, self.row) }) else {
            ui.toast("NO EFFECTS LOADED");
            return;
        };
        let name = name_of(effect.id());
        let mut line = Line::new();
        line.put(name).put(b": ");
        if route != effect.saved {
            // SAFETY: as above; the path is NUL terminated and lives in the runtime's module table
            let queued = unsafe { core.live(LIVE_RELOAD, core.module_path(effect.owner)) };
            if queued == E_BUSY {
                ui.toast("BUSY: TRY AGAIN");
                return;
            }
            // SAFETY: as above
            if unsafe { fxsend::save_route(core, self.row, route) } != 0 {
                ui.toast(line.put(b"NOT SAVED").str());
                return;
            }
            if queued != 0 {
                ui.toast(line.put(b"RESTART TO APPLY").str());
                return;
            }
        }
        ui.toast(line.put(route_text(route)).str());
    }

    /// The effect of list row `index`, when it still is the one that was listed.
    fn chosen<C: CoreApi>(&self, core: &C, index: usize) -> Option<(usize, Effect)> {
        let (k, owner) = *self.rows[..self.listed].get(index)?;
        // SAFETY: the UI task runs the callbacks of the service
        let effect = unsafe { fxsend::effect(core, k as usize) }?;
        (effect.owner == owner).then_some((k as usize, effect))
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
