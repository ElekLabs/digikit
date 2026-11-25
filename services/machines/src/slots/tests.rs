use super::*;
use dkrt_abi::fw::Fw;

use dkrt_test_support::FakeCore;

const V: usize = 0x1000;
const PRESS: u32 = F_PRESS;
const REL: u32 = F_RELEASE;

fn code(e: u32) -> u32 {
    FIRST_CODE + e
}

#[test]
fn press_then_release_is_a_click() {
    let mut d = Detector::new();
    assert_eq!(d.key(V, code(1), PRESS), None);
    assert_eq!(d.key(V, code(1), REL), Some(1));
    assert_eq!(d.key(V, code(1), REL), None, "one click per press");
}

#[test]
fn a_turn_between_press_and_release_is_a_coarse_step() {
    let mut d = Detector::new();
    d.key(V, code(2), PRESS);
    d.turn(V);
    assert_eq!(d.key(V, code(2), REL), None);
    // the next press starts clean
    d.key(V, code(2), PRESS);
    assert_eq!(d.key(V, code(2), REL), Some(2));
}

#[test]
fn unused_and_same_encoder_turns_cancel_click_and_flow_before_consumption() {
    for encoder in [1, 4] {
        // A itself and unused D on a replacement page
        let mut detector = Detector::new();
        let mut held = Held::new();
        held.begin(Ctx {
            view: V,
            track: 0,
            owner: 1,
        });
        detector.key(V, code(0), PRESS);
        turn_input(&mut detector, &mut held, V, encoder);
        assert_eq!(held.ctx(), None);
        assert_eq!(detector.key(V, code(0), REL), None);
        detector.key(V, code(0), PRESS);
        assert_eq!(detector.key(V, code(0), REL), Some(0));
    }
}

#[test]
fn a_turn_without_a_press_is_ignored() {
    let mut d = Detector::new();
    d.turn(V);
    d.key(V, code(0), PRESS);
    assert_eq!(d.key(V, code(0), REL), Some(0));
}

#[test]
fn repeats_neither_start_nor_break_a_click() {
    let mut d = Detector::new();
    d.key(V, code(3), PRESS);
    assert_eq!(d.key(V, code(3), F_REPEAT), None);
    assert_eq!(d.key(V, code(3), PRESS | F_REPEAT), None);
    assert_eq!(d.key(V, code(3), REL), Some(3));
    // a repeat alone does not arm a release
    assert_eq!(d.key(V, code(4), PRESS | F_REPEAT), None);
    assert_eq!(d.key(V, code(4), REL), None);
}

#[test]
fn a_release_of_another_encoder_is_not_a_click() {
    let mut d = Detector::new();
    d.key(V, code(1), PRESS);
    assert_eq!(d.key(V, code(2), REL), None);
    assert_eq!(d.key(V, code(1), REL), None, "the pending press was spent");
}

#[test]
fn another_key_or_another_view_cancels() {
    let mut d = Detector::new();
    d.key(V, code(1), PRESS);
    d.key(V, 0x02, PRESS); // SRC key
    assert_eq!(d.key(V, code(1), REL), None);
    d.key(V, code(1), PRESS);
    assert_eq!(
        d.key(V + 0x100, code(1), REL),
        None,
        "released on another page"
    );
    // a release of a key that is not an encoder's leaves the press alone
    d.key(V, code(1), PRESS);
    d.key(V, 0x02, REL);
    assert_eq!(d.key(V, code(1), REL), Some(1));
}

#[test]
fn a_chord_is_not_a_click() {
    let mut d = Detector::new();
    d.key(V, code(1), PRESS | F_CHORD);
    assert_eq!(d.key(V, code(1), REL), None);
    d.key(V, code(1), PRESS);
    assert_eq!(d.key(V, code(1), REL | F_CHORD), None);
}

#[test]
fn codes_outside_the_encoders_are_no_click() {
    let mut d = Detector::new();
    d.key(V, FIRST_CODE - 1, PRESS);
    assert_eq!(d.key(V, FIRST_CODE - 1, REL), None);
    d.key(V, FIRST_CODE + ENCODERS, PRESS);
    assert_eq!(d.key(V, FIRST_CODE + ENCODERS, REL), None);
    assert_eq!(d.key(V, 0, REL), None);
}

#[test]
fn a_click_on_a_parameter_with_an_action_gives_its_callback_and_the_track() {
    let actions = [0, 0xa0, 0, 0xb0];
    assert_eq!(resolve(&actions, 100, 101, 5), Some((0xa0, 5)));
    assert_eq!(resolve(&actions, 100, 103, 15), Some((0xb0, 15)));
}

#[test]
fn a_click_on_a_stock_knob_or_a_parameter_without_an_action_passes_through() {
    let actions = [0, 0xa0, 0, 0xb0];
    assert_eq!(resolve(&actions, 100, 100, 7), None);
    assert_eq!(
        resolve(&actions, 100, 0xcd, 7),
        None,
        "a stock parameter code (SAMP)"
    );
    assert_eq!(resolve(&actions, 100, 0, 7), None, "an empty knob");
    assert_eq!(resolve(&actions, 100, -1, 7), None);
    assert_eq!(
        resolve(&[0; 4], 100, 101, 7),
        None,
        "no module declares an action"
    );
}

#[test]
fn the_action_is_found_by_parameter_row() {
    let actions = [0, 0xa0, 0, 0xb0];
    assert_eq!(find(&actions, 100, 101), 0xa0);
    assert_eq!(find(&actions, 100, 103), 0xb0);
    assert_eq!(
        find(&actions, 100, 100),
        0,
        "a row without an action: the stock click"
    );
    assert_eq!(
        find(&actions, 100, 0xcd),
        0,
        "a stock parameter code is below the run-time rows"
    );
    assert_eq!(find(&actions, 100, 104), 0, "past the last row");
    assert_eq!(find(&actions, 100, -1), 0);
    assert_eq!(find(&actions, 100, 0), 0);
}

#[test]
fn the_click_context_keeps_the_view_the_track_and_the_owner_of_the_clicked_row() {
    let owners = [0u8, 3, 3, 5];
    let p = Pending {
        callback: 0xa0,
        track: 6,
        view: 0x2000,
        code: 101,
    };
    assert_eq!(
        context(&p, &owners, 100),
        Some(Ctx {
            view: 0x2000,
            track: 6,
            owner: 3
        })
    );
    let p = Pending { code: 103, ..p };
    assert_eq!(context(&p, &owners, 100).map(|c| c.owner), Some(5));
}

#[test]
fn a_click_whose_row_was_unloaded_has_no_context() {
    let owners = [0u8, 0, 3];
    let p = Pending {
        callback: 0xa0,
        track: 1,
        view: 0x2000,
        code: 100,
    };
    assert_eq!(context(&p, &owners, 100), None, "free row");
    assert_eq!(
        context(&Pending { code: 50, ..p }, &owners, 100),
        None,
        "a stock code"
    );
    assert_eq!(
        context(&Pending { code: 200, ..p }, &owners, 100),
        None,
        "past the table"
    );
}

#[test]
fn a_click_reads_the_parameter_rows_through_the_core() {
    let mut core = FakeCore::new();
    let mut t = std::vec![0; 96];
    t[dkrt_svc_rows::rows_info::w::P_DYN0] = 200;
    core.set_rows_info(t);
    let mut owners = [0u8; limits::PARAM_ROWS];
    owners[..4].copy_from_slice(&[0, 0, 3, 0]);
    let p = |code| Pending {
        callback: 1,
        track: 5,
        view: V,
        code,
    };
    // row 2 (code 202) belongs to module record 2 (owner 3)
    assert_eq!(
        click_context(&core, &owners, &p(202)),
        Some(Ctx {
            view: V,
            track: 5,
            owner: 3
        })
    );
    // a free row, a code below the first row and a code past the table have no context
    assert_eq!(click_context(&core, &owners, &p(201)), None);
    assert_eq!(click_context(&core, &owners, &p(199)), None);
    assert_eq!(click_context(&core, &owners, &p(204)), None);
    assert!(core.calls().is_empty(), "reading rows writes nothing");
}

/// The key hook asks the page for the clicked encoder's code in a step of its own, outside the owner (`dks_page_key`): the page's code hook enters the owner to look up a native page,
/// which an enclosing call would refuse. So the detector half asks the firmware nothing, and the queue half takes the code the caller got.
#[test]
fn the_key_hook_halves_ask_the_firmware_between_them_and_not_inside() {
    use dkrt_test_support::FakeFw;
    let mut core = FakeCore::new();
    let mut t = std::vec![0; 96];
    t[dkrt_svc_rows::rows_info::w::P_DYN0] = 200;
    core.set_rows_info(t);
    let fw = FakeFw {
        track: 5,
        ..FakeFw::default()
    };
    let mut slots = Slots::new();
    slots.set(2, 0x4000);
    let view = [0u32; 64];
    let v = view.as_ptr() as *const u8;
    let ev = |flags: u32| [0u32, 0, 0, code(1), flags, 0];
    let (press, release) = (ev(PRESS), ev(REL));
    unsafe {
        assert_eq!(slots.key_click(v, press.as_ptr() as *const u8), None);
        assert_eq!(slots.key_click(v, release.as_ptr() as *const u8), Some(1));
        assert!(
            fw.calls().is_empty(),
            "detecting the click asks the firmware nothing"
        );
        slots.queue_click(&core, v, 202, fw.view_track(0));
    }
    let p = slots.pending();
    assert_eq!((p.callback, p.track, p.code), (0x4000, 5, 202));
}
