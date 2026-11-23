//! The FX ROUTING steps over `FakeCore` and a fake `Ui`: the list text, the routes offered, what a pick writes and queues, and the toasts.
use super::*;
use crate::fxpage;
use dkrt_sdk::{E_RELOAD_RETAINED, OK};
use dkrt_test_support::FakeCore;
use std::string::{String, ToString};
use std::sync::MutexGuard;
use std::vec::Vec;

const MAGIC: u32 = 0x4658_5047;
const DEVICE: u32 = 1;

/// The service's statics for one test: the table bound, the rows a test gives, and all of it cleared again when the test ends (also when it fails).
struct Arena {
    _lock: MutexGuard<'static, ()>,
    core: FakeCore,
}

impl Arena {
    /// `cells` send cells in the image's table.
    fn new(cells: u32) -> Arena {
        let lock = crate::test_lock::hold();
        let mut t = std::vec![0u32; 64];
        t[0] = MAGIC;
        t[1] = 4;
        t[2] = DEVICE;
        t[4] = 0x2c;
        t[5] = 14;
        t[6] = 3;
        t[8] = 8;
        t[15] = 32;
        t[37] = 70;
        t[38] = 0x46a7;
        t[39] = cells;
        t[61] = 13;
        t[63] = 0x80;
        let mut core = FakeCore::new();
        core.device = DEVICE;
        core.info_tables.push((fxpage::INFO_HASH, t));
        // SAFETY: the lock is held
        unsafe {
            assert_eq!(fxpage::bind(&core, [0, 0]), OK);
        }
        Arena { _lock: lock, core }
    }

    /// Effect row `k` of module `id` (record `k`, so owner `k + 1`) that allows `allow`, saved as `saved` (None: no entry; the default is master).
    fn effect(&self, k: usize, id: &str, allow: u8, saved: Option<u8>) {
        let s = fxsend_state();
        s.id[k] = [0; 32];
        s.id[k][..id.len()].copy_from_slice(id.as_bytes());
        s.idlen[k] = id.len() as u8;
        s.allow[k] = allow;
        s.dflt[k] = MASTER;
        // SAFETY: the lock is held
        unsafe { (*core::ptr::addr_of_mut!(fxpage::DKRT_FXP)).owner[k] = k as u8 + 1 };
        let mut path = std::format!("modules/{id}.dkm").into_bytes();
        path.push(0);
        let mut loaded = self.core.loaded.borrow_mut();
        while loaded.len() <= k {
            loaded.push((std::vec![0], std::vec![0]));
        }
        loaded[k] = (id.bytes().chain([0]).collect(), path);
        if let Some(v) = saved {
            self.core
                .settings
                .borrow_mut()
                .push((std::format!("fx/route/{id}").into_bytes(), std::vec![v]));
        }
    }

    fn setting(&self, id: &str) -> Option<Vec<u8>> {
        let key = std::format!("fx/route/{id}").into_bytes();
        self.core
            .settings
            .borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: the lock is still held
        unsafe {
            fxpage::unbind();
            *core::ptr::addr_of_mut!(fxpage::DKRT_FXP) = core::mem::zeroed();
            *core::ptr::addr_of_mut!(crate::fxsend::DKRT_FXS) = core::mem::zeroed();
        }
    }
}

fn fxsend_state() -> &'static mut crate::fxsend::SendState {
    // SAFETY: the tests hold the lock
    unsafe { &mut *core::ptr::addr_of_mut!(crate::fxsend::DKRT_FXS) }
}

const MASTER: u8 = dkneeds::page::DK_ROUTE_MASTER as u8;
const SEND: u8 = dkneeds::page::DK_ROUTE_SEND as u8;
const BOTH: u8 = MASTER | SEND;

/// What the steps showed.
#[derive(Default)]
struct FakeUi {
    choices: Vec<(String, Vec<String>, Step)>,
    toasts: Vec<String>,
}

impl Ui for FakeUi {
    fn choice(&mut self, title: &Line, items: &[Line], step: Step) -> bool {
        self.choices.push((
            title.as_str().to_string(),
            items.iter().map(|l| l.as_str().to_string()).collect(),
            step,
        ));
        true
    }
    fn toast(&mut self, text: &Line) {
        self.toasts.push(text.as_str().to_string());
    }
}

fn items(ui: &FakeUi) -> Vec<&str> {
    ui.choices[0].1.iter().map(String::as_str).collect()
}

#[test]
fn the_list_has_a_row_for_each_effect_with_its_name_and_saved_route() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    a.effect(2, "fx/gain-send", BOTH, Some(1));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    assert_eq!(ui.choices.len(), 1);
    assert_eq!(ui.choices[0].0, "FX ROUTING");
    assert_eq!(ui.choices[0].2, Step::Effect);
    assert_eq!(items(&ui), ["FREQSHIFT: MASTER", "GAIN-SEND: SEND"]);
}

#[test]
fn a_long_name_is_cut_to_twelve_so_a_row_stays_within_twenty() {
    let a = Arena::new(2);
    a.effect(0, "someauthor/very-long-effect-name", BOTH, Some(0));
    a.effect(1, "x/abcdefghijklmnop", BOTH, Some(1));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    assert_eq!(items(&ui), ["VERY-LONG-EF: MASTER", "ABCDEFGHIJKL: SEND"]);
    assert!(items(&ui).iter().all(|i| i.len() <= 20));
}

#[test]
fn an_id_without_a_slash_is_the_name() {
    let a = Arena::new(2);
    a.effect(0, "plain", BOTH, Some(1));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    assert_eq!(items(&ui), ["PLAIN: SEND"]);
}

#[test]
fn no_effect_loaded_gives_a_toast_and_no_list() {
    let a = Arena::new(2);
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    assert!(ui.choices.is_empty());
    assert_eq!(ui.toasts, ["NO EFFECTS LOADED"]);
}

#[test]
fn the_routes_offered_are_send_then_master_for_an_effect_that_allows_both() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    routing.pick_effect(&a.core, &mut ui, 0);
    let (title, routes, step) = &ui.choices[1];
    assert_eq!(title, "FREQSHIFT ROUTE");
    assert_eq!(routes, &["SEND", "MASTER"]);
    assert_eq!(*step, Step::Route);
}

#[test]
fn send_is_not_offered_when_every_send_cell_is_taken_by_other_effects() {
    let a = Arena::new(1);
    a.effect(0, "fx/first", BOTH, Some(1));
    a.effect(1, "fx/second", BOTH, Some(0));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    // the effect that holds the cell keeps both; the other one only has master
    routing.pick_effect(&a.core, &mut ui, 0);
    routing.pick_effect(&a.core, &mut ui, 1);
    assert_eq!(ui.choices[1].1, ["SEND", "MASTER"]);
    assert_eq!(ui.choices[2].1, ["MASTER"]);
}

#[test]
fn a_send_only_effect_with_no_cell_free_offers_nothing_and_says_so() {
    let a = Arena::new(1);
    a.effect(0, "fx/first", BOTH, Some(1));
    a.effect(1, "fx/sendonly", SEND, None);
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    routing.pick_effect(&a.core, &mut ui, 1);
    assert_eq!(ui.choices.len(), 1);
    assert_eq!(ui.toasts, ["SENDONLY: NO ROUTE FREE"]);
}

/// The list, the effect at `index`, the route at `route`.
fn pick(a: &Arena, ui: &mut FakeUi, index: usize, route: usize) {
    let mut routing = Routing::new();
    routing.list(&a.core, ui);
    routing.pick_effect(&a.core, ui, index);
    routing.pick_route(&a.core, ui, route);
}

#[test]
fn a_changed_route_is_saved_and_the_effects_module_is_reloaded() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    let mut ui = FakeUi::default();
    pick(&a, &mut ui, 0, 0);
    assert_eq!(a.setting("fx/freqshift"), Some(std::vec![1]));
    assert_eq!(
        *a.core.live_calls.borrow(),
        [(LIVE_RELOAD, b"modules/fx/freqshift.dkm".to_vec())]
    );
    // the reload is only queued: the toast follows its end
    assert!(ui.toasts.is_empty());
}

#[test]
fn the_same_route_writes_nothing_and_queues_nothing() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(1));
    let mut ui = FakeUi::default();
    // SEND (index 0) is the saved route
    pick(&a, &mut ui, 0, 0);
    assert!(a.core.live_calls.borrow().is_empty());
    assert!(!a
        .core
        .calls()
        .iter()
        .any(|c| matches!(c, dkrt_test_support::Call::SettingsPut(..))));
    assert_eq!(ui.toasts, ["FREQSHIFT: SEND"]);
}

#[test]
fn a_refused_reload_keeps_the_saved_route_for_the_next_boot() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(1));
    a.core.live_result.set(E_RELOAD_RETAINED);
    let mut ui = FakeUi::default();
    // MASTER is index 1
    pick(&a, &mut ui, 0, 1);
    assert_eq!(a.setting("fx/freqshift"), Some(std::vec![0]));
    assert_eq!(ui.toasts, ["FREQSHIFT: RESTART TO APPLY"]);
}

#[test]
fn a_running_job_saves_nothing_so_the_pick_can_be_made_again() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(1));
    a.core.live_result.set(E_BUSY);
    let mut ui = FakeUi::default();
    pick(&a, &mut ui, 0, 1);
    assert_eq!(a.setting("fx/freqshift"), Some(std::vec![1]));
    assert_eq!(ui.toasts, ["BUSY: TRY AGAIN"]);
    // the job is over: the same pick takes effect
    a.core.live_result.set(0);
    let mut ui = FakeUi::default();
    pick(&a, &mut ui, 0, 1);
    assert_eq!(a.setting("fx/freqshift"), Some(std::vec![0]));
    assert!(ui.toasts.is_empty());
}

#[test]
fn a_failed_save_says_so() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    a.core.put_result.set(-1);
    let mut ui = FakeUi::default();
    pick(&a, &mut ui, 0, 0);
    assert_eq!(ui.toasts, ["FREQSHIFT: NOT SAVED"]);
}

#[test]
fn an_effect_replaced_meanwhile_is_not_picked() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    // another module takes the row while the list is open
    unsafe { (*core::ptr::addr_of_mut!(fxpage::DKRT_FXP)).owner[0] = 9 };
    routing.pick_effect(&a.core, &mut ui, 0);
    assert_eq!(ui.choices.len(), 1);
    assert_eq!(ui.toasts, ["NO EFFECTS LOADED"]);
}

#[test]
fn a_failed_save_queues_no_reload() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    a.core.put_result.set(-1);
    let mut ui = FakeUi::default();
    pick(&a, &mut ui, 0, 0);
    assert!(a.core.live_calls.borrow().is_empty());
}

#[test]
fn the_events_of_the_ui_callbacks_run_the_steps() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(0));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.run(&a.core, &mut ui, Event::List);
    routing.run(&a.core, &mut ui, Event::Effect(0));
    routing.run(&a.core, &mut ui, Event::Route(0));
    assert_eq!(ui.choices.len(), 2);
    assert_eq!(a.core.live_calls.borrow().len(), 1);
}

#[test]
fn a_name_that_is_not_ascii_shows_as_question_marks() {
    let mut line = Line::new();
    line.put("caf\u{e9}".as_bytes());
    assert_eq!(line.as_str(), "CAF??");
}

/// A pick of MASTER for the effect saved as SEND, its reload queued; `routing` waits for the job.
fn queued_pick(a: &Arena, ui: &mut FakeUi) -> Routing {
    let mut routing = Routing::new();
    routing.list(&a.core, ui);
    routing.pick_effect(&a.core, ui, 0);
    routing.pick_route(&a.core, ui, 1);
    routing
}

const E_PLAYING: i32 = dkrt_sdk::E_RELOAD_PLAYING;

#[test]
fn the_success_toast_waits_for_the_end_of_the_reload_job() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(1));
    let mut ui = FakeUi::default();
    let mut routing = queued_pick(&a, &mut ui);
    // queued or running: nothing yet
    a.core.live_job.set([1, 0, 2, 0, LIVE_RELOAD]);
    routing.poll_job(&a.core, &mut ui);
    assert!(ui.toasts.is_empty());
    a.core.live_job.set([2, 0, 6, 0, LIVE_RELOAD]);
    routing.poll_job(&a.core, &mut ui);
    routing.poll_job(&a.core, &mut ui);
    assert_eq!(ui.toasts, ["FREQSHIFT: MASTER"], "once");
    assert!(a.core.reload_when_possible.borrow().is_empty());
}

#[test]
fn a_reload_refused_by_the_playing_transport_is_deferred_to_stop() {
    let a = Arena::new(2);
    a.effect(0, "fx/freqshift", BOTH, Some(1));
    let mut ui = FakeUi::default();
    let mut routing = queued_pick(&a, &mut ui);
    a.core
        .live_job
        .set([2, E_PLAYING as u32, 6, 0, LIVE_RELOAD]);
    routing.poll_job(&a.core, &mut ui);
    assert_eq!(ui.toasts, ["FREQSHIFT: MASTER ON STOP"]);
    assert_eq!(
        *a.core.reload_when_possible.borrow(),
        [b"fx/freqshift".to_vec()]
    );
    assert_eq!(
        a.setting("fx/freqshift"),
        Some(std::vec![0]),
        "the key stays saved"
    );
}

#[test]
fn a_reload_the_job_refuses_for_another_reason_is_no_success() {
    for refusal in [E_RELOAD_RETAINED, dkrt_sdk::E_RELOAD_MENUS] {
        let a = Arena::new(2);
        a.effect(0, "fx/freqshift", BOTH, Some(1));
        let mut ui = FakeUi::default();
        let mut routing = queued_pick(&a, &mut ui);
        a.core.live_job.set([2, refusal as u32, 1, 0, LIVE_RELOAD]);
        routing.poll_job(&a.core, &mut ui);
        assert_eq!(ui.toasts, ["FREQSHIFT: RESTART TO APPLY"]);
        assert!(a.core.reload_when_possible.borrow().is_empty());
    }
}

#[test]
fn the_longest_toast_fits_the_line() {
    // a 12 character name, the longest route, the deferred text
    let mut line = Line::new();
    line.put(b"abcdefghijkl")
        .put(b": ")
        .put(route_text(Route::Master))
        .put(ON_STOP);
    assert_eq!(line.as_str(), "ABCDEFGHIJKL: MASTER ON STOP");
}
