//! Host tests of the `fx` service over `FakeCore`: the handlers that need no firmware page (record refusal, the row words in publishing order, the clear
//! sequence). The page rows, the send cells and the kit tables are written through `FwMem`: `tests/contract.rs` loads and unloads whole effects and compares them
//! (the contract suite); the device tests (`core/fxpages`) cover the firmware's own use of them.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{
    FXW_ENABLED, FXW_P0, FXW_PC, FX_FREE_PC, FX_ROW_WORDS, F_FIRST, MIX_ONE, WIN_FX,
};
use dkrt_abi::service::{tick_phase, ApplyCtx, Service};
use dkrt_sdk::{E_NO_REGISTRY, OK};
use dkrt_svc_fx::fxpage;
use dkrt_svc_fx::records::Fx;
use dkrt_test_support::{Call, FakeCore};

// A `FakeCore` has no `fxpages_info` table (`info_tables` is empty): an image without effect pages.

const REC: usize = 3;
const FIRST_ROW: u32 = 6;

/// A core whose `dynfx_info` says the effect table starts at row `FIRST_ROW`, with effect row 1 claimed by module record `REC` and published.
fn core_with_row() -> FakeCore {
    let mut core = FakeCore::new();
    core.fx_table[F_FIRST] = FIRST_ROW;
    let d = core.dsp();
    d.fx_owner[1] = REC as u8 + 1;
    d.fx_pub[1] = 1;
    d.fx_pc[1] = 0x1234;
    core
}

/// An image without effect pages refuses an effect record before it writes anything, and its tick does nothing: the fx service runs over a fake core.
#[test]
fn without_effect_pages_a_record_is_refused_and_the_tick_is_silent() {
    let core = core_with_row();
    let rec = [0u8; fxpage::LEN_EFFECT as usize];
    let ctx = ApplyCtx {
        owner: REC as u8 + 1,
        reg: core::ptr::null(),
        base: 0,
    };
    unsafe {
        assert_eq!(
            <Fx as Service<FakeCore>>::apply_record(&core, fxpage::KIND_EFFECT, rec.as_ptr(), &ctx),
            E_NO_REGISTRY
        );
        assert_eq!(
            <Fx as Service<FakeCore>>::apply_record(&core, 999, rec.as_ptr(), &ctx),
            OK,
            "a kind of another service is not its business"
        );
        <Fx as Service<FakeCore>>::tick(&core, tick_phase::LATE);
        <Fx as Service<FakeCore>>::remove(&core, REC as u8 + 1);
    }
    assert!(
        core.calls().is_empty(),
        "no parameter row, no link write: {:?}",
        core.calls()
    );
}

/// A declared effect takes its flags and defaults on its claimed row, and the loader reads its words in the row's publishing order.
#[test]
fn a_declared_effect_publishes_its_words_in_order() {
    let core = core_with_row();
    let par = [10, 11, 12, 13, 14, 15, 16, 17];
    unsafe {
        // effect ordinal 0 of the module is the claimed row 1; ordinal 1 does not exist
        assert_eq!(
            dkrt_svc_fx::dsp::fx_set_declared(&core, REC, 0, Some(2), Some(par)),
            true
        );
        assert_eq!(
            dkrt_svc_fx::dsp::fx_set_declared(&core, REC, 1, Some(2), None),
            false
        );
        let d = core.dsp();
        assert_eq!((d.fx_flags[1], d.fx_par[1]), (2, par));
        let at = FX_ROW_WORDS * (FIRST_ROW + 1);
        let step = |n: u32| {
            let mut want = n;
            <Fx as Service<FakeCore>>::dsp_row_step(&core, REC, &mut want, true)
                .map(|w| (w.win, w.word, w.value))
        };
        assert_eq!(step(0), Some((WIN_FX, at + FXW_P0, 10)));
        assert_eq!(step(7), Some((WIN_FX, at + FXW_P0 + 7, 17)));
        assert_eq!(step(11), Some((WIN_FX, at + FXW_ENABLED, 1)));
        assert_eq!(
            step(17),
            Some((WIN_FX, at + FXW_PC, 0x1234)),
            "the pc goes last: it makes the row visible"
        );
        assert!(step(18).is_none(), "the row has 18 steps");
        assert_eq!(
            <Fx as Service<FakeCore>>::attribute(
                &core,
                dkrt_sdk::fault::SITE_FX_MASTER,
                FIRST_ROW + 1
            ),
            Some(REC)
        );
    }
}

/// Clearing the rows of a module sends one sequence for the row (the pc to "free" first), drops its live values and frees the row.
#[test]
fn clearing_a_module_sends_the_free_pc_first_and_frees_the_row() {
    let core = core_with_row();
    unsafe {
        assert!(<Fx as Service<FakeCore>>::dsp_clear(&core, REC));
        let at = FX_ROW_WORDS * (FIRST_ROW + 1);
        let wire = core.link.borrow().t.words();
        assert_eq!(
            wire.first().map(|w| (w.0, w.1, w.2)),
            Some((WIN_FX, at + FXW_PC, FX_FREE_PC))
        );
        assert!(wire
            .iter()
            .any(|w| w.1 == at + dkrt_abi::dsp::FXW_MIX && w.2 == MIX_ONE));
        assert_eq!(core.dsp().fx_pub[1], 0);
        // nothing published any more: a second clear sends nothing
        let n = wire.len();
        assert!(<Fx as Service<FakeCore>>::dsp_clear(&core, REC));
        assert_eq!(core.link.borrow().t.words().len(), n);
        <Fx as Service<FakeCore>>::dsp_release(&core, REC);
        assert_eq!(core.dsp().fx_owner[1], 0);
    }
    assert!(!core.calls().iter().any(|c| matches!(c, Call::Log { .. })));
}

/// A reload waits while an effect of the module is on a route, and only that module's: an effect with no route, or another module's route, does not refuse it.
#[test]
fn a_reload_waits_while_an_effect_of_the_module_is_on_a_route() {
    let core = core_with_row();
    let owner = REC as u8 + 1;
    let ask = |who: u8| unsafe { <Fx as Service<FakeCore>>::reload_refusal(&core, who) };
    assert_eq!(ask(owner), OK, "the effect is published but on no route");
    core.dsp().fx_route[1] = 1;
    assert_eq!(ask(owner), dkrt_sdk::E_RELOAD_ROUTED);
    assert_eq!(
        ask(owner + 1),
        OK,
        "another module's reload is not held up by it"
    );
    core.dsp().fx_route[1] = 0;
    assert_eq!(ask(owner), OK);
}
