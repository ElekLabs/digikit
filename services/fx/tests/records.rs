//! Host tests of the `fx` service over `FakeCore`: the handlers that need no firmware page (record refusal, the row words in publishing order, the clear
//! sequence). The page rows, the send cells and the kit tables are written through `FwMem`: `tests/contract.rs` loads and unloads whole effects and compares them
//! (the contract suite); the device tests (`core/fxpages`) cover the firmware's own use of them.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{FXW_ENABLED, FXW_P0, FXW_PC, FX_FREE_PC, FX_ROW_WORDS, MIX_ONE, WIN_FX};
use dkrt_abi::service::{tick_phase, ApplyCtx, Service};
use dkrt_sdk::{E_NO_REGISTRY, OK};
use dkrt_svc_fx::fxpage;
use dkrt_svc_fx::records::Fx;
use dkrt_svc_fx::rows::frows;
use dkrt_test_support::contract::locked;
use dkrt_test_support::{Call, FakeCore};
use std::sync::MutexGuard;

// A `FakeCore` has no `fxpages_info` table (`info_tables` is empty): an image without effect pages.

const REC: usize = 3;
const FIRST_ROW: u32 = 6;

/// A core whose `dynfx_info` says the effect table starts at row `FIRST_ROW`, with effect row 1 claimed by module record `REC` and published.
fn core_with_row() -> (MutexGuard<'static, ()>, FakeCore) {
    let rows_lock = locked();
    dkrt_svc_fx::rows::reset();
    let mut core = FakeCore::new();
    core.set_dynfx(FIRST_ROW, 4);
    let d = frows();
    d.fx_owner[1] = REC as u8 + 1;
    d.fx_pub[1] = 1;
    d.fx_pc[1] = 0x1234;
    (rows_lock, core)
}

/// An image without effect pages refuses an effect record before it writes anything, and its tick does nothing: the fx service runs over a fake core.
#[test]
fn without_effect_pages_a_record_is_refused_and_the_tick_is_silent() {
    let (_rows, core) = core_with_row();
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
    let (_rows, core) = core_with_row();
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
        let d = frows();
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
                // the byte offset of the row in the effect table, as the DSP reports it
                4 * FX_ROW_WORDS * (FIRST_ROW + 1)
            ),
            Some(REC)
        );
    }
}

/// Clearing the rows of a module sends one sequence for the row (the pc to "free" first), drops its live values and frees the row.
#[test]
fn clearing_a_module_sends_the_free_pc_first_and_frees_the_row() {
    let (_rows, core) = core_with_row();
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
        assert_eq!(frows().fx_pub[1], 0);
        // nothing published any more: a second clear sends nothing
        let n = wire.len();
        assert!(<Fx as Service<FakeCore>>::dsp_clear(&core, REC));
        assert_eq!(core.link.borrow().t.words().len(), n);
        <Fx as Service<FakeCore>>::dsp_release(&core, REC);
        assert_eq!(frows().fx_owner[1], 0);
    }
    assert!(!core.calls().iter().any(|c| matches!(c, Call::Log { .. })));
}

/// An effect is torn down on any route (route 0 is the master chain) and is never what holds up a reload or an unload: the service refuses nothing, for its own
/// module or another's, published row or not.
#[test]
fn a_published_effect_row_never_refuses_a_teardown() {
    let (_rows, core) = core_with_row();
    let owner = REC as u8 + 1;
    let ask = |who: u8| unsafe { <Fx as Service<FakeCore>>::teardown_refusal(&core, who) };
    for route in [0, 1] {
        frows().fx_route[1] = route;
        assert_eq!(ask(owner), OK, "route {route}");
    }
    assert_eq!(ask(owner + 1), OK);
    frows().fx_pub[1] = 0;
    assert_eq!(ask(owner), OK);
}

/// The rows the module being reloaded owns count as free in the check of its new file (the teardown that precedes the load frees them), and the rows of another module
/// do not: with all four rows taken, a file of four effects fits the reload of their owner and no other load.
#[test]
fn the_rows_of_the_module_being_reloaded_count_as_free_in_the_row_check() {
    use dkrt_abi::dsp::DspSec;
    let (_rows, mut core) = core_with_row();
    core.set_dynfx(FIRST_ROW, 4);
    let d = frows();
    for k in 0..4 {
        d.fx_owner[k] = if k < 3 { REC as u8 + 1 } else { REC as u8 + 2 };
    }
    let sec = |nfx: u32| DspSec {
        s: core::ptr::null(),
        version: 5,
        size: 0,
        image_size: 0,
        image_off: 0,
        reloc_off: 0,
        nreloc: 0,
        import_off: 0,
        nimport: 0,
        voice_off: 0,
        nvoice: 0,
        fx_off: 0,
        nfx,
        vstride: 0,
        bank_off: 0,
    };
    let run = |nfx: u32, live_op: u32| {
        core.live_op.set(live_op);
        let r = unsafe { <Fx as Service<FakeCore>>::dsp_check_rows(&core, &sec(nfx)) };
        core.live_op.set(0);
        r
    };
    let owner = REC as u32 + 1;
    assert_eq!(run(1, 0), dkrt_sdk::E_DSP_ROWS, "no free row");
    assert_eq!(run(3, 0), dkrt_sdk::E_DSP_ROWS, "no reload runs");
    assert_eq!(run(3, owner), OK, "the module's own three rows");
    assert_eq!(
        run(4, owner),
        dkrt_sdk::E_DSP_ROWS,
        "the fourth row is another module's"
    );
    assert_eq!(
        run(3, owner + 1),
        dkrt_sdk::E_DSP_ROWS,
        "one row of the reloaded module is not enough"
    );
}
