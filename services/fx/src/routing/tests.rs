//! The FX ROUTING steps over `FakeCore` and a fake `Ui`: the list text, the routes offered, what a pick writes and queues, and the toasts.
use super::*;
use crate::fxpage;
use dkrt_sdk::{E_RELOAD_RETAINED, OK};
use dkrt_test_support::FakeCore;
use std::string::{String, ToString};
use std::sync::MutexGuard;
use std::vec::Vec;

const DEVICE: u32 = 1;

/// The service's statics for one test: the table bound, the rows a test gives, and all of it cleared again when the test ends (also when it fails).
struct Arena {
    _lock: MutexGuard<'static, ()>,
    core: FakeCore,
}

impl Arena {
    /// `cells` send cells on the device.
    fn new(cells: u32) -> Arena {
        let lock = crate::test_lock::hold();
        let mut core = FakeCore::new();
        core.device = DEVICE;
        core.set_rows_blocks(&[(2, 40, 24, 0x75), (3, 90, 2, 0x75)]);
        let facts = fxpage::DeviceFacts {
            device: DEVICE,
            page_first: 14,
            slots: 3,
            values_n: 32,
            mark_index: 70,
            send_cells: cells,
            knob_block: 2,
            send_block: 3,
            track_page: 13,
            store_off: 0x80,
            ..Default::default()
        };
        // SAFETY: the lock is held
        unsafe {
            assert_eq!(fxpage::bind_facts(&core, &facts, [0, 0]), OK);
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
    /// A dialog is already open: `choice` shows nothing.
    busy: bool,
}

impl Ui for FakeUi {
    fn choice(&mut self, title: &Line, items: &[Line], step: Step) -> bool {
        if self.busy {
            return false;
        }
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
    assert_eq!(items(&ui), ["FREQSHIFT\t: MASTER", "GAIN-SEND\t: SEND"]);
}

#[test]
fn a_name_that_fits_shows_whole_in_the_list_and_a_longer_one_is_cut_in_the_middle() {
    let a = Arena::new(2);
    a.effect(0, "x/nineteen-characters", BOTH, Some(0));
    a.effect(1, "x/abcdefghijklmnopqrstuvwxyz", BOTH, Some(1));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    // 19 characters stay; 26 become 19 with `..` in the middle
    assert_eq!(
        items(&ui),
        [
            "NINETEEN-CHARACTERS\t: MASTER",
            "ABCDEFGHI..STUVWXYZ\t: SEND"
        ]
    );
}

#[test]
fn names_that_differ_only_at_the_end_stay_apart_when_cut() {
    let (mut one, mut two) = (Line::new(), Line::new());
    one.put_fit(b"long-effect-name-one", 13);
    two.put_fit(b"long-effect-name-two", 13);
    assert_eq!(
        (one.as_str(), two.as_str()),
        ("LONG-E..E-ONE", "LONG-E..E-TWO")
    );
}

#[test]
fn the_title_and_the_toasts_keep_a_long_name_within_their_limits() {
    let a = Arena::new(1);
    a.effect(0, "x/a-very-long-effect-nam", BOTH, Some(1));
    a.effect(1, "x/a-very-long-sendonly-nam", SEND, None);
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    routing.pick_effect(&a.core, &mut ui, 0);
    assert_eq!(ui.choices[1].0, "A-VERY-L..ECT-NAM ROUTE");
    assert!(ui.choices[1].0.len() <= 23, "DK_UI_ROW");
    routing.pick_effect(&a.core, &mut ui, 1);
    assert_eq!(ui.toasts, ["A-VERY..Y-NAM: NO ROUTE FREE"]);
    assert!(ui.toasts[0].len() <= 31, "DK_UI_TOAST");
}

#[test]
fn a_pick_saves_nothing_when_the_row_changed_owner_since_the_effect_was_chosen() {
    let a = Arena::new(2);
    a.effect(0, "fx/gain-send", BOTH, Some(MASTER));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    routing.list(&a.core, &mut ui);
    routing.pick_effect(&a.core, &mut ui, 0);
    // the module of the row was reloaded (or replaced) while the popup was open: the row has a new owner
    // SAFETY: the lock is held
    unsafe { (*core::ptr::addr_of_mut!(fxpage::DKRT_FXP)).owner[0] = 7 };
    routing.pick_route(&a.core, &mut ui, 1);
    assert_eq!(
        a.setting("fx/gain-send"),
        Some(std::vec![MASTER]),
        "not saved"
    );
    assert_eq!(ui.toasts, ["BUSY: TRY AGAIN"]);
}

#[test]
fn a_list_or_effect_pick_says_busy_when_a_dialog_is_already_open() {
    let a = Arena::new(2);
    a.effect(0, "fx/gain-send", BOTH, Some(MASTER));
    let (mut routing, mut ui) = (Routing::new(), FakeUi::default());
    ui.busy = true;
    routing.list(&a.core, &mut ui);
    assert_eq!(ui.toasts, ["BUSY: TRY AGAIN"]);
    routing.pick_effect(&a.core, &mut ui, 0);
    assert_eq!(ui.toasts, ["BUSY: TRY AGAIN", "BUSY: TRY AGAIN"]);
}
