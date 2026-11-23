use super::*;
use crate::dsp as fxdsp;
use dkrt_abi::core_api::CoreApi;
use dkrt_test_support::{Call, FakeCore};

/// A knob of effect row 2 is declared with a default, then its value changes: the parameter row is written first, then the new value is the word's live value.
#[test]
fn a_knob_gets_its_row_then_a_change_is_set_on_the_link() {
    let mut s: std::boxed::Box<FxpState> = unsafe { std::boxed::Box::new(core::mem::zeroed()) };
    let mut core = FakeCore::new();
    core.set_dynfx(0, 0);
    // the parameter table of the firmware, as a fake: rows of 0x40 bytes at 0x1000, the mirror index in the second word
    let mut t = std::vec![0; 96];
    t[dkrt_svc_rows::dynreg::w::P_TABLE] = 0x1000;
    t[dkrt_svc_rows::dynreg::w::PF_SIZE] = 0x40;
    core.set_dynreg(t);
    unsafe {
        s.min[2][1] = 0f32.to_bits();
        s.span[2][1] = 1f32.to_bits();
        write_knob_row(&core, &mut s, 2, 1, 120, 7, 33, 0x4000);
        assert_eq!(
            (s.cur[2][1], s.def[2][1], s.dsp[2][1], s.want[2][1]),
            (0x4000, 0x4000, 0x4000, 0x4000)
        );
        assert!(set_knob(&core, &mut s, 2, 1, 0x7f00));
        assert_eq!(s.want[2][1], 0x7f00);
        // a knob is word 4 + j of its row, MIX word 13, whatever its place among the knobs; the rows are 24 words apart
        set_knob(&core, &mut s, 3, WORDS - 1, 0x7f00);
    }
    assert_eq!(
        unsafe { dkrt_abi::mem::FwMem::rd(&core, 0x1000 + 120 * 0x40 + 4) },
        33,
        "the effect knob's row has the kit's value index as its mirror index"
    );
    let calls: std::vec::Vec<Call> = core
        .calls()
        .into_iter()
        .filter(|c| !matches!(c, Call::Wr(..)))
        .collect();
    assert!(matches!(
        calls[0],
        Call::Set {
            slot: dkrt_abi::link::Slot { win: 2, word: 53 },
            ..
        }
    ));
    assert!(matches!(
        calls[1],
        Call::Set {
            slot: dkrt_abi::link::Slot { win: 2, word: 85 },
            ..
        }
    ));
}

/// The offsets that the device tests read from `DKRT_FXP` (`tests/regression/runtime/fx-pages-ui.ts`).
#[test]
fn the_layout_the_device_tests_read_is_unchanged() {
    assert_eq!(core::mem::offset_of!(FxpState, sent), 16);
    assert_eq!(core::mem::offset_of!(FxpState, errors), 20);
    assert_eq!(core::mem::offset_of!(FxpState, seeds), 24);
    assert_eq!(core::mem::offset_of!(FxpState, owner), 28);
    assert_eq!(core::mem::offset_of!(FxpState, cur), 36);
    assert_eq!(core::mem::offset_of!(FxpState, dsp), 292);
}

/// Several turns before the link is free end as one write of the newest value, and the word counts as the DSP's when the link confirms it.
#[test]
fn a_fast_turn_sends_the_last_value_and_the_dsp_value_follows_the_confirmation() {
    let mut s: std::boxed::Box<FxpState> = unsafe { std::boxed::Box::new(core::mem::zeroed()) };
    let mut core = FakeCore::new();
    core.set_dynfx(0, 0);
    unsafe {
        s.min[2][1] = 0f32.to_bits();
        s.span[2][1] = 1f32.to_bits();
        write_knob_row(&core, &mut s, 2, 1, 120, 7, 33, 0x4000);
        for n in [0x100, 0x200, 0x300] {
            set_knob(&core, &mut s, 2, 1, n);
        }
        for _ in 0..50 {
            core.link.borrow_mut().pump();
        }
        let slot = fxdsp::fx_slot(&core, 2, 5).unwrap();
        let wire = core.link.borrow().t.words();
        assert_eq!(wire.len(), 1);
        assert_eq!(
            core.link_confirmed(slot),
            Some(knob_value(s.min[2][1], s.span[2][1], 0x300))
        );
    }
}
