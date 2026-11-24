//! Host tests of the `fx` service over `FakeCore`: the handlers that need no firmware page (record refusal, the row words in publishing order, the clear
//! sequence). The page rows, the send cells and the kit tables are written through `FwMem`: `tests/contract.rs` loads and unloads whole effects and compares them
//! (the contract suite); the device tests (`core/fxpages`) cover the firmware's own use of them.
use dkrt_abi::dsp::{FXW_ENABLED, FXW_P0, FXW_PC, FX_FREE_PC, FX_ROW_WORDS, MIX_ONE, WIN_FX};
use dkrt_abi::provider::{Consumer, ConsumerId, DspPart, Provider, Records, Ticker};
use dkrt_abi::service::tick_phase;
use dkrt_sdk::{E_FORMAT, E_NO_REGISTRY, OK};
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::fxpage;
use dkrt_svc_fx::rows::frows;
use dkrt_test_support::contract::locked;
use dkrt_test_support::{Call, FakeCore};
use std::sync::MutexGuard;

// A `FakeCore` has no `fxpages_info` table (`info_tables` is empty): an image without effect pages.

const REC: usize = 3;
const FIRST_ROW: u32 = 6;

/// A core whose `dynfx_info` says the effect table starts at row `FIRST_ROW`, with effect row 1 claimed by module record `REC` and published, and an owner that
/// has not bound a table (no `fxpages_info` here).
fn core_with_row() -> (MutexGuard<'static, ()>, FakeCore, Fx) {
    let rows_lock = locked();
    dkrt_svc_fx::rows::reset();
    let mut core = FakeCore::new();
    core.set_dynfx(FIRST_ROW, 4);
    let d = frows();
    d.fx_owner[1] = REC as u8 + 1;
    d.fx_pub[1] = 1;
    d.fx_pc[1] = 0x1234;
    (rows_lock, core, Fx::new())
}

/// An image without effect pages refuses an effect record before it writes anything, and its tick does nothing: the fx service runs over a fake core.
#[test]
fn without_effect_pages_a_record_is_refused_and_the_tick_is_silent() {
    let (_rows, core, mut fx) = core_with_row();
    let rec = [0u8; fxpage::LEN_EFFECT as usize];
    let consumer = Consumer::new(ConsumerId::new(REC as u8 + 1), &[]);
    assert_eq!(
        Records::apply(&mut fx, &core, fxpage::KIND_EFFECT, &rec, &consumer),
        Err(E_NO_REGISTRY)
    );
    assert_eq!(
        Records::apply(&mut fx, &core, 999, &rec, &consumer),
        Ok(()),
        "a kind of another service is not its business"
    );
    assert_eq!(
        Records::apply(&mut fx, &core, fxpage::KIND_EFFECT, &rec[..8], &consumer),
        Err(E_FORMAT),
        "a body shorter than a record is refused before it is read"
    );
    Ticker::tick(&mut fx, &core, tick_phase::LATE);
    Provider::detach(&mut fx, &core, ConsumerId::new(REC as u8 + 1));
    assert!(
        core.calls().is_empty(),
        "no parameter row, no link write: {:?}",
        core.calls()
    );
}

/// A declared effect takes its flags and defaults on its claimed row, and the loader reads its words in the row's publishing order.
#[test]
fn a_declared_effect_publishes_its_words_in_order() {
    let (_rows, core, mut fx) = core_with_row();
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
        let mut step = |n: u32| {
            let mut want = n;
            DspPart::row_step(&mut fx, &core, REC, &mut want, true)
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
            DspPart::attribute(
                &mut fx,
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
    let (_rows, core, mut fx) = core_with_row();
    unsafe {
        assert!(DspPart::clear(&mut fx, &core, REC));
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
        assert!(DspPart::clear(&mut fx, &core, REC));
        assert_eq!(core.link.borrow().t.words().len(), n);
        DspPart::release(&mut fx, &core, REC);
        assert_eq!(frows().fx_owner[1], 0);
    }
    assert!(!core.calls().iter().any(|c| matches!(c, Call::Log { .. })));
}

/// An effect is torn down on any route (route 0 is the master chain) and is never what holds up a reload or an unload: the service refuses nothing, for its own
/// module or another's, published row or not.
#[test]
fn a_published_effect_row_never_refuses_a_teardown() {
    let (_rows, core, mut fx) = core_with_row();
    let owner = REC as u8 + 1;
    // the owner keeps the default `can_stop`: it holds up nothing, whoever goes down
    let ask = |fx: &Fx| Provider::can_stop(fx, &core);
    for route in [0, 1] {
        frows().fx_route[1] = route;
        assert_eq!(ask(&fx), Ok(()), "route {route} of module {owner}");
    }
    assert_eq!(ask(&fx), Ok(()));
    frows().fx_pub[1] = 0;
    assert_eq!(ask(&fx), Ok(()));
}

/// The rows the module being reloaded owns count as free in the check of its new file (the teardown that precedes the load frees them), and the rows of another module
/// do not: with all four rows taken, a file of four effects fits the reload of their owner and no other load.
#[test]
fn the_rows_of_the_module_being_reloaded_count_as_free_in_the_row_check() {
    use dkrt_abi::dsp::DspSec;
    let (_rows, mut core, mut fx) = core_with_row();
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
    let mut run = |nfx: u32, live_op: u32| {
        core.live_op.set(live_op);
        let r = DspPart::check_rows(&mut fx, &core, &sec(nfx));
        core.live_op.set(0);
        r
    };
    let owner = REC as u32 + 1;
    assert_eq!(run(1, 0), Err(dkrt_sdk::E_DSP_ROWS), "no free row");
    assert_eq!(run(3, 0), Err(dkrt_sdk::E_DSP_ROWS), "no reload runs");
    assert_eq!(run(3, owner), Ok(()), "the module's own three rows");
    assert_eq!(
        run(4, owner),
        Err(dkrt_sdk::E_DSP_ROWS),
        "the fourth row is another module's"
    );
    assert_eq!(
        run(3, owner + 1),
        Err(dkrt_sdk::E_DSP_ROWS),
        "one row of the reloaded module is not enough"
    );
}

// ---- Records::check -------------------------------------------------------------------------------------------------------------------

/// A `register` section of `n` effect records of zero bodies, as the registry walker hands it to `check`.
fn section(n: usize) -> Vec<u8> {
    let mut s = vec![0u8; 16];
    s[12..16].copy_from_slice(&(n as u32).to_be_bytes());
    for _ in 0..n {
        let mut rec = vec![0u8; fxpage::LEN_EFFECT as usize];
        rec[0..4].copy_from_slice(&fxpage::KIND_EFFECT.to_be_bytes());
        rec[4..8].copy_from_slice(&fxpage::LEN_EFFECT.to_be_bytes());
        s.extend(rec);
    }
    s
}

fn run_check(fx: &mut Fx, core: &FakeCore, step: u32, reg: &[u8]) -> Result<(), i32> {
    let c = dkrt_abi::service::CheckCtx {
        reg: reg.as_ptr(),
        image_size: 0x4000,
    };
    Records::check(fx, core, step, &c)
}

/// The count step refuses more effect records than the chain has rows, and accepts as many as it has; a step the service does not know accepts.
#[test]
fn the_count_step_refuses_more_effects_than_the_chain_takes() {
    use dkrt_abi::service::check_step;
    let (_rows, core, mut fx) = core_with_row();
    let max = dkrt_abi::dsp::MAX_FX;
    assert_eq!(
        run_check(&mut fx, &core, check_step::COUNTS, &section(max)),
        Ok(())
    );
    assert_eq!(
        run_check(&mut fx, &core, check_step::COUNTS, &section(max + 1)),
        Err(E_FORMAT)
    );
    assert_eq!(
        run_check(&mut fx, &core, check_step::ROWS, &section(max + 1)),
        Ok(())
    );
}

/// The effects step needs the effect pages of the image (`E_NO_REGISTRY` without them, also for a section with records only), and then the rule of each record; a section
/// without effect records needs neither.
#[test]
fn the_effects_step_needs_the_pages_and_the_rule_of_each_record() {
    use dkrt_abi::service::check_step;
    let _lock = dkrt_test_support::contract::locked();
    unsafe { fxpage::unbind() };
    let mut fx = Fx::new();
    let core = FakeCore::new();
    assert_eq!(
        run_check(&mut fx, &core, check_step::EFFECTS, &section(0)),
        Ok(())
    );
    assert_eq!(
        run_check(&mut fx, &core, check_step::EFFECTS, &section(1)),
        Err(E_NO_REGISTRY)
    );
    // with the table of the image, a record of zero bytes breaks the rule (no name, no knob rows)
    let mut with = FakeCore::new();
    with.device = 1;
    let mut t = vec![0u32; 64];
    t[0] = 0x4658_5047;
    t[1] = 4;
    t[2] = 1;
    t[4] = 0x2c;
    t[6] = 3;
    t[8] = 8;
    t[15] = 32;
    t[63] = 0x80;
    with.info_tables.push((fxpage::INFO_HASH, t));
    assert_eq!(
        run_check(&mut fx, &with, check_step::EFFECTS, &section(1)),
        Err(E_FORMAT)
    );
    unsafe { fxpage::unbind() };
}
