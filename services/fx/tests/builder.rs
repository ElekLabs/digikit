//! The builder calls of the effects and their commit over a `FakeCore`: the collection, its refusals, the lowering to the effect records of the `register` section (and the
//! effects' lines of `tests/fixtures/records/lowered.golden`, recorded before the collect moved into the services) and the passes of `Records::commit`. Each test makes its own
//! owner or builder: the collection is not shared.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::provider::Records;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
use dkrt_sdk::*;
use dkrt_svc_fx::builder::*;
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::fxpage::{KIND_EFFECT, LEN_EFFECT};
use dkrt_test_support::contract::locked;
use dkrt_test_support::FakeCore;
use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;

fn c(s: &'static [u8]) -> *const u8 {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr()
}

const FXID: u32 = 0x1234;
const DK_BIPOLAR: u32 = 2;

fn keys(fk: &[u32]) -> KeysCopy {
    let mut k = KeysCopy::empty();
    k.nf = fk.len();
    k.fk[..fk.len()].copy_from_slice(fk);
    k
}

fn rd(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// A builder whose `init` of module record `rec` is open, and the core that runs that module's code.
fn opened(rec: usize) -> (Box<Builder>, FakeCore) {
    let core = FakeCore::new();
    core.running.set(rec as u8 + 1);
    core.source.set(rec as u8 + 1);
    let mut b = Box::new(Builder::new());
    b.begin(rec);
    (b, core)
}

/// An effect (the ordinal `index` of the module) with a page of one bipolar knob.
fn effect(index: u32, mix: Option<u32>) -> Box<Builder> {
    let (mut b, core) = opened(0);
    unsafe {
        let fx = b.add_fx(&core, 4 as *mut u8, index, c(b"Freq shift\0"), c(b"FSH\0"));
        b.fx_routes(&core, fx, ROUTE_SEND | ROUTE_MASTER, ROUTE_SEND);
        let pg = b.fx_add_page(&core, fx, c(b"Freq shift\0"), c(b"FSH\0"));
        b.page_add_fx_param(
            &core,
            pg,
            c(b"Freq Shift\0"),
            c(b"SHIFT\0"),
            DK_BIPOLAR,
            -64.0,
            63.0,
            0.0,
        );
        if let Some(m) = mix {
            b.fx_mix(&core, fx, m);
        }
    }
    b
}

/// The effect records `lower` writes (header words not written) and how many.
fn lowered(b: &Build, k: &KeysCopy) -> (Vec<u8>, u32) {
    let mut measured = RecordSink::measure();
    lower(b, k, &mut measured).unwrap();
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    lower(b, k, &mut out).unwrap();
    assert_eq!(
        (out.at, out.count),
        (measured.at, measured.count),
        "the writing pass has the size and count the measuring pass found"
    );
    (buf, out.count)
}

#[test]
fn a_call_outside_init_returns_the_placeholder_and_changes_nothing() {
    let core = FakeCore::new();
    let mut b = Box::new(Builder::new());
    let ph = b.placeholder();
    unsafe {
        b.end();
        assert_eq!(
            b.add_fx(&core, core::ptr::null_mut(), 0, c(b"A\0"), c(b"A\0")),
            ph
        );
        assert_eq!(b.fx_add_page(&core, ph, c(b"A\0"), c(b"S\0")), ph);
        b.fx_routes(&core, ph, 1, 1);
        b.fx_mix(&core, ph, 5);
        // inside init a context that is not the running module's is no builder either
        b.begin(0);
        assert_eq!(
            b.add_fx(&core, core::ptr::null_mut(), 0, c(b"A\0"), c(b"A\0")),
            ph
        );
        assert_eq!((b.build().nf, b.build().err), (0, 0));
        // the running module's context opens an effect, which takes a page and its knob
        core.running.set(1);
        core.source.set(1);
        let f = b.add_fx(&core, 4 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
        assert_ne!(f, ph);
        let p = b.fx_add_page(&core, f, c(b"A\0"), c(b"S\0"));
        assert_ne!(
            b.page_add_fx_param(&core, p, c(b"K\0"), c(b"K\0"), 1, 0.0, 1.0, 0.5),
            ph
        );
        b.end();
        assert_eq!(b.fx_add_page(&core, f, c(b"A\0"), c(b"S\0")), ph);
    }
}

#[test]
fn a_handle_of_another_service_on_an_effect_page_is_refused() {
    let (mut b, core) = opened(0);
    let foreign = 0x1234 as *mut u8;
    unsafe {
        let p = b.page_add_fx_param(&core, foreign, c(b"K\0"), c(b"K\0"), 1, 0.0, 1.0, 0.5);
        assert_eq!(p, b.placeholder());
    }
    assert_eq!(
        b.build().err,
        E_FORMAT,
        "a machine's page takes no effect parameter"
    );
    // the placeholder of this service is not a foreign handle: no error of its own
    let (mut b, core) = opened(0);
    let ph = b.placeholder();
    unsafe { b.page_add_fx_param(&core, ph, c(b"K\0"), c(b"K\0"), 1, 0.0, 1.0, 0.5) };
    assert_eq!(b.build().err, 0);
}

#[test]
fn an_effect_short_name_longer_than_its_tab_label_is_refused_not_cut() {
    let (mut b, core) = opened(0);
    unsafe { b.add_fx(&core, 4 as *mut u8, 0, c(b"Freq shift\0"), c(b"FSHF\0")) };
    assert_eq!(b.build().err, E_FORMAT);
    assert_eq!(check_commit(b.build(), &keys(&[FXID])), E_FORMAT);
    // the page's tab label too, and an effect has one page
    let (mut b, core) = opened(0);
    unsafe {
        let f = b.add_fx(&core, 4 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
        b.fx_add_page(&core, f, c(b"A\0"), c(b"ABCD\0"));
    }
    assert_eq!(b.build().err, E_FORMAT);
    let (mut b, core) = opened(0);
    unsafe {
        let f = b.add_fx(&core, 4 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
        b.fx_add_page(&core, f, c(b"A\0"), c(b"A\0"));
        b.fx_add_page(&core, f, c(b"B\0"), c(b"B\0"));
    }
    assert_eq!(b.build().err, E_FORMAT);
}

#[test]
fn commit_refuses_an_effect_index_the_keys_table_lacks() {
    let b = effect(0, None);
    let f = b.build();
    assert_eq!(f.err, 0);
    assert_eq!(check_commit(f, &keys(&[FXID])), OK);
    assert_eq!(check_commit(f, &keys(&[])), E_DECL_ID);
    // an effect id that is twice in the file or in another module is the planner's refusal (E_PLAN_KEY), not the commit's
    assert_eq!(check_commit(f, &keys(&[FXID, FXID])), OK);
}

#[test]
fn an_effect_with_a_page_lowers_to_an_effect_record_in_keys_order() {
    let b = effect(1, None);
    let f = b.build();
    let (buf, count) = lowered(f, &keys(&[0x77, FXID]));
    assert_eq!((count, buf.len()), (1, 16 + 0x1a4));
    let e = 16;
    assert_eq!(
        (
            rd(&buf, e),
            rd(&buf, e + 8),
            rd(&buf, e + 12),
            rd(&buf, e + 16),
            rd(&buf, e + 20)
        ),
        (4, 1, 127, 1, FXID)
    );
    assert_eq!(&buf[e + 24..e + 27], b"FSH");
    assert_eq!(fx_flags(ROUTE_SEND | ROUTE_MASTER, ROUTE_SEND), 0b101);
    assert_eq!(fx_flags(ROUTE_SEND, ROUTE_SEND), 0b111);
    assert_eq!(f32::from_bits(fx_defaults(f, 0)[0]), 0.0);
    // the display of a bipolar knob is 1
    assert_eq!(rd(&buf, e + 8 + 48 + 4), 1);
}

#[test]
fn dk_fx_mix_sets_the_mix_default_of_the_effect_record_and_127_without_it() {
    let k = keys(&[0x77, FXID]);
    assert_eq!(rd(&lowered(effect(1, None).build(), &k).0, 16 + 12), 127);
    assert_eq!(rd(&lowered(effect(1, Some(64)).build(), &k).0, 16 + 12), 64);
}

#[test]
fn an_effect_without_routes_runs_on_the_master_only_and_one_with_routes_follows_them() {
    let (mut f, core) = opened(0);
    unsafe {
        f.add_fx(&core, 4 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
        let b = f.add_fx(&core, 4 as *mut u8, 1, c(b"B\0"), c(b"B\0"));
        let d = f.add_fx(&core, 4 as *mut u8, 2, c(b"D\0"), c(b"D\0"));
        f.fx_routes(&core, b, ROUTE_SEND | ROUTE_MASTER, ROUTE_SEND);
        f.fx_routes(&core, d, ROUTE_SEND, ROUTE_SEND);
    }
    // no flag bit: master only, default master (the committed row keeps the old fixture's rule)
    let fxs = &f.build().fxs;
    assert_eq!(fx_flags_of_decl(&fxs[0]), 0);
    assert_eq!(fx_flags_of_decl(&fxs[1]), 0b101);
    assert_eq!(fx_flags_of_decl(&fxs[2]), 0b111);
}

// ---- the commit over a FakeCore ------------------------------------------------------------------------------------------------------

const REC: usize = 3;
const FIRST_ROW: u32 = 6;

/// A core whose `dynfx_info` says the effect table starts at row `FIRST_ROW`, with effect row 1 claimed by module record `REC` (the row the effect of ordinal 0 declares to).
fn core() -> (MutexGuard<'static, ()>, FakeCore, Fx) {
    let rows_lock = locked();
    dkrt_svc_fx::rows::reset();
    let mut core = FakeCore::new();
    core.set_dynfx(FIRST_ROW, 4);
    let d = dkrt_svc_fx::rows::frows();
    d.fx_owner[1] = REC as u8 + 1;
    (rows_lock, core, Fx::new())
}

fn commit(
    fx: &mut Fx,
    core: &FakeCore,
    rec: usize,
    phase: u32,
    k: &KeysCopy,
    sink: &mut RecordSink,
) -> Result<(), i32> {
    let ctx = ModuleCommitCtx {
        phase,
        base: 0,
        image_size: 0x4000,
        keys: k,
        sink,
    };
    Records::commit(fx, core, rec, &ctx)
}

fn collect_effect(fx: &mut Fx, core: &FakeCore, rec: usize) {
    unsafe {
        Records::begin(fx, core, rec);
        core.running.set(rec as u8 + 1);
        core.source.set(rec as u8 + 1);
        let b = fx.builder();
        let f = b.add_fx(core, 4 as *mut u8, 0, c(b"Freq shift\0"), c(b"FSH\0"));
        b.fx_routes(core, f, ROUTE_SEND | ROUTE_MASTER, ROUTE_SEND);
        let p = b.fx_add_page(core, f, c(b"Freq shift\0"), c(b"FSH\0"));
        b.page_add_fx_param(
            core,
            p,
            c(b"Freq Shift\0"),
            c(b"SHIFT\0"),
            DK_BIPOLAR,
            -64.0,
            63.0,
            0.5,
        );
    }
}

#[test]
fn the_commit_measures_then_writes_and_declares_the_effects_row() {
    let (_rows, core, mut fx) = core();
    collect_effect(&mut fx, &core, REC);
    let k = keys(&[FXID]);
    let mut measured = RecordSink::measure();
    assert_eq!(
        commit(
            &mut fx,
            &core,
            REC,
            commit_phase::PREPARE,
            &k,
            &mut measured
        ),
        Ok(())
    );
    assert_eq!((measured.count, measured.at), (1, 16 + LEN_EFFECT as usize));
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    assert_eq!(
        commit(&mut fx, &core, REC, commit_phase::WRITE, &k, &mut out),
        Ok(())
    );
    assert_eq!((out.count, out.at, out.rows), (1, measured.at, [0; 3]));
    assert_eq!(rd(&buf, 16), KIND_EFFECT);
    // nothing of the DSP row is declared before the declare pass (the walker's check comes in between)
    assert_eq!(dkrt_svc_fx::rows::frows().fx_flags[1], 0);
    assert_eq!(
        commit(&mut fx, &core, REC, commit_phase::DECLARE, &k, &mut out),
        Ok(())
    );
    // the effect's DSP row has the route flags and the knob default of the builder calls
    let d = dkrt_svc_fx::rows::frows();
    assert_eq!(
        (d.fx_flags[1], f32::from_bits(d.fx_par[1][0])),
        (0b101, 0.5)
    );
}

#[test]
fn a_module_without_effects_commits_nothing_and_a_provider_adds_none() {
    let core = FakeCore::new();
    let mut fx = Fx::new();
    let k = keys(&[]);
    Records::begin(&mut fx, &core, 1);
    let mut sink = RecordSink::measure();
    assert_eq!(
        commit(&mut fx, &core, 1, commit_phase::PREPARE, &k, &mut sink),
        Ok(())
    );
    assert_eq!((sink.count, sink.at), (0, RecordSink::HEADER));
    // a provider that collected an effect
    let mut fx = Fx::new();
    collect_effect(&mut fx, &core, dkrt_abi::MAX_MODULES);
    assert_eq!(
        commit(
            &mut fx,
            &core,
            dkrt_abi::MAX_MODULES,
            commit_phase::PREPARE,
            &keys(&[FXID]),
            &mut RecordSink::measure()
        ),
        Err(E_FORMAT)
    );
}

// ---- golden lowered records -----------------------------------------------------------------------------------------------------

// The effects' lines of `tests/fixtures/records/lowered.golden`: the EFFECT record of each case and what the commit takes from the collection for the effect's DSP row (the route
// flags and the knob defaults). The machines crate has the test for the machines' lines of the same file.

fn hex(b: &[u8]) -> std::string::String {
    b.iter().map(|x| std::format!("{x:02x}")).collect()
}

fn golden_case(name: &str, b: &Builder, k: &KeysCopy) -> std::string::String {
    let f = b.build();
    use std::fmt::Write;
    let mut s = std::string::String::new();
    writeln!(s, "case {name}").unwrap();
    if !f.is_empty() {
        let (buf, _) = lowered(f, k);
        let mut at = 16;
        while at < buf.len() {
            let (kind, len) = (rd(&buf, at), rd(&buf, at + 4) as usize);
            writeln!(s, "record {kind} {}", hex(&buf[at..at + len])).unwrap();
            at += len;
        }
    }
    for i in 0..f.nf {
        let d = fx_defaults(f, i);
        writeln!(
            s,
            "fx {i} flags {:08x} defaults {}",
            fx_flags_of_decl(&f.fxs[i]),
            d.iter()
                .map(|w| std::format!("{w:08x}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .unwrap();
    }
    s
}

/// `tests/fixtures/modules/commit-demo/src/module.c`: the effect with routes and one page.
fn commit_demo() -> Box<Builder> {
    let (mut f, core) = opened(0);
    unsafe {
        let fx = f.add_fx(&core, 4 as *mut u8, 0, c(b"Commit FX\0"), c(b"CFX\0"));
        f.fx_routes(&core, fx, ROUTE_SEND | ROUTE_MASTER, ROUTE_SEND);
        let fp = f.fx_add_page(&core, fx, c(b"Commit FX\0"), c(b"CFX\0"));
        f.page_add_fx_param(&core, fp, c(b"Gain\0"), c(b"GAIN\0"), 1, 0.0, 1.27, 0.5);
    }
    assert_eq!(f.build().err, 0);
    f
}

fn golden_text() -> std::string::String {
    let none = Builder::new();
    let mut s = std::string::String::new();
    for case in [
        "dt2-analog",
        "dn2-analog",
        "dn2-shot-canvas-pages",
        "dn2-v1-canvas-page",
        "dn2-sine-knob-page",
    ] {
        s += &golden_case(case, &none, &keys(&[]));
    }
    let fk = keys(&[0x77, FXID]);
    s += &golden_case("dt2-effect", &effect(1, None), &fk);
    s += &golden_case("dt2-effect-mix-64", &effect(1, Some(64)), &fk);
    s += &golden_case("dt2-commit-demo", &commit_demo(), &keys(&[FXID]));
    s += &golden_case("dn2-commit-demo", &commit_demo(), &keys(&[FXID]));
    s
}

/// The lines of the golden file that belong to this service: the case headers, the effect records and the effects' DSP lines.
fn mine(line: &str) -> bool {
    line.starts_with("case ") || line.starts_with("fx ") || line.starts_with("record 4 ")
}

#[test]
fn the_lowered_records_of_the_golden_cases_are_the_recorded_bytes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/records/lowered.golden");
    let text = golden_text();
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let want: Vec<&str> = want.lines().filter(|l| mine(l)).collect();
    let have: Vec<&str> = text.lines().collect();
    if let Some(at) = (0..want.len().max(have.len())).find(|i| want.get(*i) != have.get(*i)) {
        panic!("the lowered records differ from {} at the effects' line {}:\n  now:      {}\n  recorded: {}", path.display(), at + 1, have.get(at).unwrap_or(&"(end)"), want.get(at).unwrap_or(&"(end)"));
    }
}
