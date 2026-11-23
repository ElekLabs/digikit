//! The builder calls of the machines and their commit over a `FakeCore`: the collection, its refusals, the lowering to the records of the `register` section (the records the packer
//! wrote before builders, and `tests/fixtures/records/lowered.golden`, recorded before the collect moved into the services), and the two passes of `module_commit`.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink, Service};
use dkrt_sdk::records::*;
use dkrt_sdk::*;
use dkrt_svc_machines::build::{self, *};
use dkrt_svc_machines::records::Machines;
use dkrt_svc_rows::dynreg::w;
use dkrt_test_support::FakeCore;
use std::boxed::Box;
use std::sync::Mutex;
use std::vec::Vec;

/// The tests that drive the statics of the crate (the open collection) take this lock.
static LOCK: Mutex<()> = Mutex::new(());

fn c(s: &'static [u8]) -> *const u8 {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr()
}

const KNOB_PARAM: u32 = 0x8000_0000;
const DT2: Profile = Profile {
    device: 1,
    pages: false,
    slot_base: 0,
    clone_of: 0,
    level: 10,
    own_dsp: false,
    tune_code: 0xca,
    tune_slot: 0,
    level_code: 0xd1,
    level_knob: 7,
    nslots: 6,
    slots: [1, 2, 4, 5, 6, 7, 0, 0],
    tpl: [0xcc, 0xd1, 0x47, 0x32, 0xca, 0xcb],
};
const DN2: Profile = Profile {
    device: 2,
    pages: true,
    slot_base: 25,
    clone_of: 0,
    level: 10,
    own_dsp: true,
    tune_code: 0,
    tune_slot: 0,
    level_code: 0,
    level_knob: 0,
    nslots: 8,
    slots: [1, 2, 4, 5, 6, 0, 3, 7],
    tpl: [0xd5, 0xd7, 0x47, 0xdd, 0xee, 0xd5],
};
const KEY: u32 = 0x00ab_cd00;
const FREE: Free = Free {
    machine_rows: 8,
    param_rows: 16,
    page_rows: 8,
};

fn keys(mk: &[u32], rows: (u32, u32, u32)) -> KeysCopy {
    let mut k = KeysCopy::empty();
    k.machine_rows = rows.0;
    k.param_rows = rows.1;
    k.page_rows = rows.2;
    k.nm = mk.len();
    k.mk[..mk.len()].copy_from_slice(mk);
    k
}

fn rd(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// A machine with three params on one page.
fn analog() -> Box<Build> {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"Analog\0"), c(b"ANLG\0"));
    let p = b.machine_add_page(m, c(b"Analog\0"), c(b"S\0"));
    b.page_add_param(p, c(b"Tone\0"), c(b"TONE\0"), 64);
    let decay = b.page_add_param(p, c(b"Decay\0"), c(b"DECAY\0"), 80);
    b.param_range(decay, 0, 100);
    b.param_latched(decay);
    let play = b.page_add_param(p, c(b"Play\0"), c(b"PLAY\0"), 0);
    b.param_range(play, 0, 3);
    b.param_glyph(play, 5);
    assert_eq!(b.err, 0);
    b
}

/// The records `lower` writes for the collection (header words not written) and how many; the plan.
fn lowered(b: &Build, p: &Profile, k: &KeysCopy, base: u32) -> (Vec<u8>, u32, Plan) {
    let mut plan = Plan::new();
    let mut measured = RecordSink::measure();
    lower(b, p, k, &[true; MAX_M], base, &mut measured, &mut plan).unwrap();
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    lower(b, p, k, &[true; MAX_M], base, &mut out, &mut plan).unwrap();
    assert_eq!(
        (out.at, out.count),
        (measured.at, measured.count),
        "the writing pass has the size and count the measuring pass found"
    );
    (buf, out.count, plan)
}

fn lower_err(b: &Build, p: &Profile, k: &KeysCopy) -> Result<(), i32> {
    let mut sink = RecordSink::measure();
    lower(b, p, k, &[true; MAX_M], 0, &mut sink, &mut Plan::new())
}

#[test]
fn a_call_outside_init_returns_the_placeholder_and_changes_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = FakeCore::new();
    unsafe {
        build::end();
        assert_eq!(
            build::add_machine(&core, core::ptr::null_mut(), 0, c(b"A\0"), c(b"A\0")),
            placeholder()
        );
        assert_eq!(
            build::machine_add_page(&core, placeholder(), c(b"A\0"), c(b"S\0")),
            placeholder()
        );
        assert_eq!(
            build::page_add_param(&core, placeholder(), c(b"A\0"), c(b"A\0"), 1),
            placeholder()
        );
        build::param_range(&core, placeholder(), 1, 2);
        build::param_latched(&core, placeholder());
        build::page_bind_local(&core, placeholder(), 0, 1);
        // inside init a context that is not the running module's is no builder either (`module_of` answers 0)
        build::begin(0);
        assert_eq!(
            build::add_machine(&core, core::ptr::null_mut(), 0, c(b"A\0"), c(b"A\0")),
            placeholder()
        );
        assert_eq!(
            (build::build().nm, build::build().err),
            (0, 0),
            "a call with a context that is not live records nothing"
        );
        // the running module's context opens a machine
        core.running.set(1);
        core.source.set(1);
        let m = build::add_machine(&core, 4 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
        assert_ne!(m, placeholder());
        assert_eq!(build::build().nm, 1);
        // the code of another module (or of no module) that holds the handle changes nothing while this module's `init` is open
        core.source.set(2);
        assert_eq!(
            build::machine_add_page(&core, m, c(b"A\0"), c(b"S\0")),
            placeholder()
        );
        assert_eq!(build::build().np, 0);
        core.source.set(1);
        // the collection is closed: the handle is no more taken
        build::end();
        assert_eq!(
            build::machine_add_page(&core, m, c(b"A\0"), c(b"S\0")),
            placeholder()
        );
        assert_eq!(build::build().np, 0);
    }
}

#[test]
fn a_ninth_knob_is_an_error_kept_for_the_commit() {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"A\0"), c(b"A\0"));
    let p = b.machine_add_page(m, c(b"A\0"), c(b"S\0"));
    for _ in 0..limits::PAGE_KNOBS {
        assert_ne!(b.page_add_param(p, c(b"P\0"), c(b"P\0"), 0), placeholder());
    }
    assert_eq!(b.err, 0);
    b.page_add_param(p, c(b"P\0"), c(b"P\0"), 0);
    assert_eq!(b.err, E_FORMAT);
}

#[test]
fn the_collection_is_bounded_by_max_records_and_the_call_past_it_returns_the_placeholder() {
    let mut b = Box::new(Build::new());
    let mut machines = std::vec::Vec::new();
    for _ in 0..MAX_M {
        machines.push(b.add_machine(0, c(b"A\0"), c(b"A\0")));
    }
    let mut first = core::ptr::null_mut();
    for i in 0..MAX_PG {
        let g = b.machine_add_page(machines[i % MAX_M], c(b"G\0"), c(b"S\0"));
        if i == 0 {
            first = g;
        }
    }
    for _ in 0..(MAX_PA - MAX_M - MAX_PG) {
        assert_ne!(
            b.page_add_param(first, c(b"P\0"), c(b"P\0"), 0),
            placeholder()
        );
    }
    assert_eq!(b.err, 0, "exactly MAX_PA records fit");
    assert_eq!(b.records(), MAX_PA);
    let over = b.page_add_param(first, c(b"P\0"), c(b"P\0"), 0);
    assert_eq!(over, placeholder());
    assert_eq!(b.err, E_DECL_FULL);
    // setters on it and a call that takes it change nothing
    let before = (b.np, b.nparam, b.records());
    b.param_range(over, 5, 6);
    b.param_latched(over);
    b.param_on_click(over, 4, 8);
    b.page_canvas(over, 1, 4, 6, 8);
    b.page_bind_local(over, 0, 0);
    b.page_bind(over, over, 0);
    assert_eq!(
        b.page_add_param(over, c(b"X\0"), c(b"X\0"), 1),
        placeholder()
    );
    assert_eq!((b.np, b.nparam, b.records()), before);
    // a 17th page is the same
    assert_eq!(
        b.machine_add_page(machines[0], c(b"G\0"), c(b"S\0")),
        placeholder()
    );
}

#[test]
fn a_bad_text_is_an_error_at_commit_not_a_failed_call() {
    let mut b = Box::new(Build::new());
    let h = b.add_machine(
        0,
        c(b"A very long machine name that does not fit\0"),
        c(b"A\0"),
    );
    assert_ne!(h, placeholder());
    assert_eq!(b.err, E_FORMAT);
    assert_eq!(
        check_commit(&b, &DT2, &keys(&[KEY], (1, 0, 0)), &FREE),
        E_FORMAT
    );
}

#[test]
fn a_handle_of_another_service_is_refused_where_a_machine_call_needs_its_own() {
    // a pointer that is no element of the collection and no placeholder: an effect's page given to `dk_page_add_param`, an effect's parameter given to `dk_param_on_click`
    let other = std::boxed::Box::new([0u8; 8]);
    let foreign = other.as_ptr() as *mut u8;
    let mut b = analog();
    assert_eq!(
        b.page_add_param(foreign, c(b"P\0"), c(b"P\0"), 0),
        placeholder()
    );
    assert_eq!(b.err, E_FORMAT);
    let mut b = analog();
    b.param_on_click(foreign, 4, 8);
    assert_eq!(b.err, E_FORMAT);
    // the placeholder itself and a null handle are what an earlier refusal gave: the error is kept already, nothing new is added
    let mut b = analog();
    assert_eq!(
        b.page_add_param(placeholder(), c(b"P\0"), c(b"P\0"), 0),
        placeholder()
    );
    b.param_on_click(core::ptr::null_mut(), 4, 8);
    assert_eq!(b.err, 0);
}

#[test]
fn commit_refuses_more_rows_than_uses_an_unknown_id_and_no_free_row() {
    let b = analog();
    let ok = keys(&[KEY], (1, 3, 1));
    assert_eq!(check_commit(&b, &DN2, &ok, &FREE), OK);
    // more rows than [uses]
    assert_eq!(
        check_commit(&b, &DN2, &keys(&[KEY], (1, 2, 1)), &FREE),
        E_DECL_USES
    );
    assert_eq!(
        check_commit(&b, &DN2, &keys(&[KEY], (0, 3, 1)), &FREE),
        E_DECL_USES
    );
    // a page device needs a page row for the machine; a machine-row device none
    assert_eq!(
        check_commit(&b, &DN2, &keys(&[KEY], (1, 3, 0)), &FREE),
        E_DECL_USES
    );
    assert_eq!(check_commit(&b, &DT2, &keys(&[KEY], (1, 3, 0)), &FREE), OK);
    // a machine index that is no row of the header's keys table
    assert_eq!(
        check_commit(&b, &DN2, &keys(&[], (1, 3, 1)), &FREE),
        E_DECL_ID
    );
    // no free row
    assert_eq!(
        check_commit(
            &b,
            &DN2,
            &ok,
            &Free {
                machine_rows: 0,
                ..FREE
            }
        ),
        E_TYPE_USED
    );
    assert_eq!(
        check_commit(
            &b,
            &DN2,
            &ok,
            &Free {
                param_rows: 2,
                ..FREE
            }
        ),
        E_PARAM_FULL
    );
    assert_eq!(
        check_commit(
            &b,
            &DN2,
            &ok,
            &Free {
                page_rows: 0,
                ..FREE
            }
        ),
        E_PAGE_FULL
    );
}

#[test]
fn a_digitakt_machine_lowers_to_the_records_the_packer_wrote() {
    let b = analog();
    let k = keys(&[KEY], (1, 3, 0));
    let (buf, count, plan) = lowered(&b, &DT2, &k, 0x1000);
    assert_eq!(
        count, 4,
        "3 params and 1 machine, no page row on a machine-row device"
    );
    assert_eq!(buf.len(), 16 + 3 * 0x50 + 0x6c);
    // PARAM 1 (Decay): the encoder row 0xcc, slot = knob position of word 1 = 2, range 0..100, default 80, raw (x 256)
    let p1 = 16 + 0x50;
    assert_eq!((rd(&buf, p1), rd(&buf, p1 + 8)), (1, 1));
    assert_eq!((rd(&buf, p1 + 12), rd(&buf, p1 + 16)), (0xcc, 2));
    assert_eq!(
        (rd(&buf, p1 + 20), rd(&buf, p1 + 24), rd(&buf, p1 + 28)),
        (0, 100 * 256, 80 * 256)
    );
    // PARAM 2 (Play): the play glyph's stock row
    assert_eq!(rd(&buf, 16 + 2 * 0x50 + 12), 0xcb);
    // MACHINE: key, clone, position 255; knobs: tune on 0, the params on 1, 2 and 4 (B C E), LEV on 7
    let m = 16 + 3 * 0x50;
    assert_eq!(
        (rd(&buf, m), rd(&buf, m + 8), rd(&buf, m + 16)),
        (2, KEY, 255),
        "kind, key, position"
    );
    assert_eq!(rd(&buf, m + 12), 0, "clone_of");
    let knobs: std::vec::Vec<u32> = (0..8).map(|j| rd(&buf, m + 8 + 20 + 4 * j)).collect();
    assert_eq!(
        knobs,
        [
            0xca,
            KNOB_PARAM,
            KNOB_PARAM | 1,
            0,
            KNOB_PARAM | 2,
            0,
            0,
            0xd1
        ]
    );
    // the voice row: the range of each knob position and the latch of word 1
    let (rng, latched) = voice_declared(&b, &plan, 0);
    assert_eq!(
        rng,
        [
            0,
            127 * 256 << 16,
            100 * 256 << 16,
            0,
            3 * 256 << 16,
            0,
            0,
            0
        ]
    );
    assert_eq!(latched, 0b10);
}

fn shot() -> Box<Build> {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"Shot\0"), c(b"SHOT\0"));
    let knobs = b.machine_add_page(m, c(b"SHOT\0"), c(b"S\0"));
    let smp = b.page_add_param(knobs, c(b"Sample\0"), c(b"SMP\0"), 0);
    let start = b.page_add_param(knobs, c(b"Start\0"), c(b"START\0"), 0);
    b.param_on_click(smp, 0x1100, 0x77);
    let ctl = b.machine_add_page(m, c(b"SHOT\0"), c(b"S\0"));
    b.page_canvas(ctl, 1, 0x1200, 0x1300, 0x88);
    b.page_bind(ctl, start, 0);
    let wave = b.machine_add_page(m, c(b"SHOT WAVE\0"), c(b"S\0"));
    b.page_canvas(wave, 2, 0x1400, 0x1500, 0x99);
    b.page_bind(wave, start, 4);
    b.page_bind_local(wave, 0, 6);
    assert_eq!(b.err, 0);
    b
}

#[test]
fn a_digitone_machine_with_canvas_pages_lowers_pages_and_native_records() {
    let b = shot();
    let (buf, count, plan) = lowered(&b, &DN2, &keys(&[KEY], (1, 2, 1)), 0x1000);
    // 2 params, 1 machine, 1 page, 1 action, 2 native pages (v2)
    assert_eq!(count, 7);
    assert_eq!(buf.len(), 16 + 2 * 0x50 + 0x6c + 0x60 + 0x10 + 2 * 0x98);
    // slot = 25 + knob position: word 0 at position 1, word 1 at position 2
    assert_eq!(rd(&buf, 16 + 8 + 8), 25 + 1);
    assert_eq!(
        rd(&buf, 16 + 12),
        0xd5,
        "the Digitone II template of the default glyph"
    );
    assert_eq!(rd(&buf, 16 + 0x50 + 8 + 8), 25 + 2);
    let page = 16 + 2 * 0x50 + 0x6c;
    assert_eq!(rd(&buf, page), 3);
    let act = page + 0x60;
    assert_eq!(
        (rd(&buf, act), rd(&buf, act + 8), rd(&buf, act + 12)),
        (5, 0, 0x100),
        "action: param 0, image offset"
    );
    let n0 = act + 0x10;
    assert_eq!(
        (
            rd(&buf, n0),
            rd(&buf, n0 + 8),
            rd(&buf, n0 + 16),
            rd(&buf, n0 + 20)
        ),
        (7, 2, 0x200, 0x300)
    );
    assert_eq!(
        (rd(&buf, n0 + 8 + 40), rd(&buf, n0 + 8 + 44)),
        (0, 1),
        "page 0, overlay flag"
    );
    let b0 = n0 + 8 + 48;
    assert_eq!(
        (rd(&buf, b0), rd(&buf, b0 + 4), rd(&buf, b0 + 8)),
        (1, 1, 0),
        "param file index 1 on encoder 0"
    );
    let n1 = n0 + 0x98;
    assert_eq!(
        (rd(&buf, n1 + 8 + 40), rd(&buf, n1 + 8 + 44)),
        (1, 2),
        "page 1, wide flag"
    );
    let b1 = n1 + 8 + 48;
    assert_eq!(
        (rd(&buf, b1 + 12), rd(&buf, b1 + 16), rd(&buf, b1 + 20)),
        (2, 0, 6),
        "local 0 on encoder 6"
    );
    let args = apply_args(&b, &plan);
    assert_eq!(
        (args.click[0], args.native[0], args.native[1]),
        (0x77, 0x88, 0x99)
    );
}

#[test]
fn one_canvas_page_without_flags_is_a_v1_page() {
    let mut b = analog();
    let m = &mut b.machines[0] as *mut CMachine as *mut u8;
    let g = b.machine_add_page(m, c(b"W\0"), c(b"S\0"));
    b.page_canvas(g, 0, 0x1400, 0x1500, 0);
    b.page_bind_local(g, 3, 5);
    let (buf, _, _) = lowered(&b, &DN2, &keys(&[KEY], (1, 3, 1)), 0);
    let n = buf.len() - 0x90;
    assert_eq!((rd(&buf, n), rd(&buf, n + 8)), (6, 1), "kind 6, version 1");
    assert_eq!(
        (
            rd(&buf, n + 8 + 40),
            rd(&buf, n + 8 + 44),
            rd(&buf, n + 8 + 48)
        ),
        (2, 3, 5)
    );
}

#[test]
fn a_flagged_canvas_page_alone_is_an_incomplete_v2_pair_that_the_registry_refuses() {
    let mut b = analog();
    let m = &mut b.machines[0] as *mut CMachine as *mut u8;
    let g = b.machine_add_page(m, c(b"W\0"), c(b"S\0"));
    b.page_canvas(g, 2, 0x1400, 0x1500, 0);
    b.page_bind_local(g, 0, 0);
    let (buf, _, _) = lowered(&b, &DN2, &keys(&[KEY], (1, 3, 1)), 0);
    let n = buf.len() - 0x98;
    assert_eq!(rd(&buf, n), 7);
    let mut set = dkrt_svc_machines::nativepage::PageSet::<8>::new();
    assert!(set.claim(7, KEY, rd(&buf, n + 8 + 40)));
    assert!(!set.complete());
}

#[test]
fn a_param_beyond_the_devices_knob_positions_or_outside_its_glyph_range_is_refused() {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"A\0"), c(b"A\0"));
    let p = b.machine_add_page(m, c(b"A\0"), c(b"S\0"));
    for _ in 0..7 {
        b.page_add_param(p, c(b"P\0"), c(b"P\0"), 0);
    }
    let k = keys(&[KEY], (1, 7, 1));
    // the Digitakt has six knob positions for words, the Digitone eight
    assert_eq!(lower_err(&b, &DT2, &k), Err(E_FORMAT));
    assert!(lower_err(&b, &DN2, &k).is_ok());
    let mut b = analog();
    b.params[0].glyph = 3; // on/off wants 0 to 1, the param has 0 to 127
    assert_eq!(lower_err(&b, &DN2, &keys(&[KEY], (1, 3, 1))), Err(E_FORMAT));
}

#[test]
fn a_second_knob_page_of_a_machine_is_refused_at_the_commit() {
    // a machine has one knob page for now: a second page without a canvas is E_FORMAT on both device kinds
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"Two\0"), c(b"TWO\0"));
    let p1 = b.machine_add_page(m, c(b"One\0"), c(b"S\0"));
    b.page_add_param(p1, c(b"Tone\0"), c(b"TONE\0"), 64);
    let p2 = b.machine_add_page(m, c(b"Two\0"), c(b"S\0"));
    b.page_add_param(p2, c(b"Decay\0"), c(b"DECAY\0"), 80);
    assert_eq!(b.err, 0, "the calls themselves succeed; the commit refuses");
    assert_eq!(lower_err(&b, &DT2, &keys(&[KEY], (1, 2, 0))), Err(E_FORMAT));
    assert_eq!(lower_err(&b, &DN2, &keys(&[KEY], (1, 2, 2))), Err(E_FORMAT));
}

#[test]
fn a_digitone_knob_page_has_the_short_name_and_title_it_was_given() {
    let b = sine();
    let (buf, _, _) = lowered(&b, &DN2, &keys(&[KEY], (1, 1, 1)), 0);
    let page = 16 + 0x50 + 0x6c;
    assert_eq!(rd(&buf, page), 3);
    assert_eq!(&buf[page + 8 + 4 * 10..][..5], b"SNE\0\0");
    assert_eq!(&buf[page + 8 + 4 * 10 + 24..][..5], b"Sine\0");
}

fn sine() -> Box<Build> {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"Sine\0"), c(b"SIN\0"));
    let pg = b.machine_add_page(m, c(b"Sine\0"), c(b"SNE\0"));
    b.page_add_param(pg, c(b"Level\0"), c(b"LEVEL\0"), 127);
    b
}

// ---- the commit over a FakeCore ------------------------------------------------------------------------------------------------------

/// The words of `dynreg_info` the commit reads: the profile of `p` (version 3) and the rows the registry has (8 machines, 16 parameters, 8 pages).
fn dynreg(p: &Profile) -> Vec<u32> {
    let mut t = std::vec![0u32; 96];
    t[w::VERSION] = 3;
    t[w::DEVICE] = p.device;
    t[w::PG_ROWS] = p.pages as u32;
    t[w::SLOT_BASE] = p.slot_base;
    t[w::P_CLONE] = p.clone_of;
    t[w::P_LEVEL] = p.level;
    t[w::P_OWN] = p.own_dsp as u32;
    t[w::P_TUNE] = p.tune_code;
    t[w::P_TUNE_SLOT] = p.tune_slot;
    t[w::P_LEVEL_CODE] = p.level_code;
    t[w::P_LEVEL_KNOB] = p.level_knob;
    t[w::P_NSLOTS] = p.nslots;
    t[w::P_SLOTS..w::P_SLOTS + 8].copy_from_slice(&p.slots);
    t[w::P_TPL..w::P_TPL + 6].copy_from_slice(&p.tpl);
    t[w::M_DYN_N] = 8;
    t[w::P_DYN_N] = 16;
    t[w::PG_DYN_N] = 8;
    t
}

fn commit(core: &FakeCore, phase: u32, k: &KeysCopy, sink: &mut RecordSink) -> i32 {
    let ctx = ModuleCommitCtx {
        phase,
        base: 0x1000,
        image_size: 0x4000,
        keys: k,
        sink,
    };
    unsafe { <Machines as Service<FakeCore>>::module_commit(core, 0, &ctx) }
}

/// Fill the open collection of the service with the builder calls of `analog`.
fn collect_analog(core: &FakeCore) {
    unsafe {
        build::begin(0);
        core.running.set(1);
        core.source.set(1);
        let m = build::add_machine(core, 8 as *mut u8, 0, c(b"Analog\0"), c(b"ANLG\0"));
        let p = build::machine_add_page(core, m, c(b"Analog\0"), c(b"S\0"));
        build::page_add_param(core, p, c(b"Tone\0"), c(b"TONE\0"), 64);
        let go = build::page_add_param(core, p, c(b"Go\0"), c(b"GO\0"), 0);
        build::param_on_click(core, go, 0x2000, 5);
    }
}

#[test]
fn the_commit_measures_then_writes_the_same_records_declares_the_rows_and_closes_the_collection() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = FakeCore::new();
    core.set_dynreg(dynreg(&DT2));
    collect_analog(&core);
    let k = keys(&[KEY], (1, 2, 0));
    let mut measured = RecordSink::measure();
    assert_eq!(commit(&core, commit_phase::PREPARE, &k, &mut measured), OK);
    // 2 params, 1 machine, 1 action
    assert_eq!(
        (measured.count, measured.at),
        (4, 16 + 2 * 0x50 + 0x6c + 0x10)
    );
    assert_eq!(
        measured.rows, [0; 3],
        "the rows are counted when they are written"
    );
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    assert_eq!(commit(&core, commit_phase::WRITE, &k, &mut out), OK);
    assert_eq!((out.count, out.at, out.rows), (4, measured.at, [1, 2, 0]));
    let act = 16 + 2 * 0x50 + 0x6c;
    assert_eq!(
        (rd(&buf, act), rd(&buf, act + 8), rd(&buf, act + 12)),
        (5, 1, 0x2000 - 0x1000),
        "the action of the second param, as an image offset"
    );
    // the callback argument waits for `apply`
    assert_eq!(build::args().map(|a| a.click[0]), Some(5));
    // the declare pass (after the walker's check) closes the collection
    assert_eq!(commit(&core, commit_phase::DECLARE, &k, &mut out), OK);
    // the collection is closed: another builder call is refused until the next `module_begin`
    unsafe {
        let m = build::add_machine(&core, 8 as *mut u8, 1, c(b"X\0"), c(b"X\0"));
        assert_eq!(m, placeholder());
    }
}

#[test]
fn the_commit_refuses_without_a_registry_an_error_of_the_calls_and_a_provider_with_machines() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let k = keys(&[KEY], (1, 2, 0));
    // no dynreg_info in the image
    let core = FakeCore::new();
    collect_analog(&core);
    assert_eq!(
        commit(&core, commit_phase::PREPARE, &k, &mut RecordSink::measure()),
        E_NO_REGISTRY
    );
    unsafe { build::end() };
    // an error of the calls is the commit's result, before anything is checked
    let mut core = FakeCore::new();
    core.set_dynreg(dynreg(&DT2));
    collect_analog(&core);
    unsafe {
        build::page_add_param(&core, core::ptr::null_mut(), c(b"P\0"), c(b"P\0"), 0);
        let m = build::add_machine(&core, 8 as *mut u8, 1, core::ptr::null(), c(b"A\0"));
        assert_ne!(m, placeholder());
    }
    assert_eq!(
        commit(&core, commit_phase::PREPARE, &k, &mut RecordSink::measure()),
        E_FORMAT
    );
    unsafe { build::end() };
    // a module that declared no machine commits nothing and needs no registry
    let core = FakeCore::new();
    unsafe { build::begin(0) };
    let mut sink = RecordSink::measure();
    assert_eq!(commit(&core, commit_phase::PREPARE, &k, &mut sink), OK);
    assert_eq!((sink.count, sink.at), (0, RecordSink::HEADER));
    // a provider adds no machine row of its own
    let mut core = FakeCore::new();
    core.set_dynreg(dynreg(&DT2));
    collect_analog(&core);
    unsafe {
        build::begin(dkrt_abi::MAX_MODULES);
        core.running.set(dkrt_abi::MAX_MODULES as u8 + 1);
        core.source.set(dkrt_abi::MAX_MODULES as u8 + 1);
        build::add_machine(&core, 8 as *mut u8, 0, c(b"A\0"), c(b"A\0"));
    }
    let ctx = ModuleCommitCtx {
        phase: commit_phase::PREPARE,
        base: 0,
        image_size: 0x4000,
        keys: &k,
        sink: &mut RecordSink::measure(),
    };
    assert_eq!(
        unsafe {
            <Machines as Service<FakeCore>>::module_commit(&core, dkrt_abi::MAX_MODULES, &ctx)
        },
        E_FORMAT
    );
    unsafe { build::end() };
}

// ---- golden lowered records -----------------------------------------------------------------------------------------------------

// `tests/fixtures/records/lowered.golden` holds the records the builder calls of these cases lowered to before the collect moved into the services, grouped by the service that
// owns each kind (machines: PARAM, MACHINE, PAGE, ACTION, native pages; fx: EFFECT). This test writes the machines' lines of every case and compares them with the file's.
// A change of the lowering changes the file on purpose: `DKRT_UPDATE_GOLDEN=1 cargo test golden` rewrites the machines' and the effects' lines of it (the fx crate has the same
// test for its own), and the diff is the review.

fn hex(b: &[u8]) -> std::string::String {
    b.iter().map(|x| std::format!("{x:02x}")).collect()
}

/// The lines of one case: its records, what the commit takes from the collection for the voice rows and the callback arguments.
fn golden_case(name: &str, b: &Build, p: &Profile, k: &KeysCopy, base: u32) -> std::string::String {
    use std::fmt::Write;
    let mut s = std::string::String::new();
    writeln!(s, "case {name}").unwrap();
    let (buf, count, plan) = if b.is_empty() {
        (std::vec![0u8; 16], 0, Plan::new())
    } else {
        lowered(b, p, k, base)
    };
    let mut at = 16;
    let mut recs = 0;
    while at < buf.len() {
        let (kind, len) = (rd(&buf, at), rd(&buf, at + 4) as usize);
        writeln!(s, "record {kind} {}", hex(&buf[at..at + len])).unwrap();
        at += len;
        recs += 1;
    }
    assert_eq!(recs, count);
    for m in 0..b.nm {
        let (rng, latched) = voice_declared(b, &plan, m);
        writeln!(
            s,
            "voice {m} {} {latched:08x}",
            rng.iter()
                .map(|w| std::format!("{w:08x}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .unwrap();
    }
    let args = apply_args(b, &plan);
    let clicks = (0..b.nparam)
        .filter(|&i| b.params[i].click != 0 && plan.file[i] != u8::MAX)
        .count();
    let natives = (0..b.np).filter(|&i| b.pages[i].canvas).count();
    let join = |v: &[u32]| {
        v.iter()
            .map(|w| std::format!("{w:08x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    writeln!(
        s,
        "args click {} native {}",
        join(&args.click[..clicks]),
        join(&args.native[..natives])
    )
    .unwrap();
    s
}

fn golden_text() -> std::string::String {
    let mut s = std::string::String::new();
    let empty = Build::new();
    let one = keys(&[KEY], (1, 3, 1));
    s += &golden_case(
        "dt2-analog",
        &analog(),
        &DT2,
        &keys(&[KEY], (1, 3, 0)),
        0x1000,
    );
    s += &golden_case("dn2-analog", &analog(), &DN2, &one, 0x1000);
    s += &golden_case(
        "dn2-shot-canvas-pages",
        &shot(),
        &DN2,
        &keys(&[KEY], (1, 2, 1)),
        0x1000,
    );
    let mut v1 = analog();
    let m = &mut v1.machines[0] as *mut CMachine as *mut u8;
    let g = v1.machine_add_page(m, c(b"W\0"), c(b"S\0"));
    v1.page_canvas(g, 0, 0x1400, 0x1500, 0);
    v1.page_bind_local(g, 3, 5);
    s += &golden_case("dn2-v1-canvas-page", &v1, &DN2, &one, 0);
    s += &golden_case(
        "dn2-sine-knob-page",
        &sine(),
        &DN2,
        &keys(&[KEY], (1, 1, 1)),
        0,
    );
    s += &golden_case("dt2-effect", &empty, &DT2, &keys(&[], (0, 0, 0)), 0);
    s += &golden_case("dt2-effect-mix-64", &empty, &DT2, &keys(&[], (0, 0, 0)), 0);
    s += &golden_case(
        "dt2-commit-demo",
        &commit_demo(),
        &DT2,
        &keys(&[KEY], (1, 3, 0)),
        0x1000,
    );
    s += &golden_case(
        "dn2-commit-demo",
        &commit_demo(),
        &DN2,
        &keys(&[KEY], (1, 3, 1)),
        0x1000,
    );
    s
}

/// `tests/fixtures/modules/commit-demo/src/module.c`: the machine with three parameters (one latched, one with a click action).
fn commit_demo() -> Box<Build> {
    let mut b = Box::new(Build::new());
    let m = b.add_machine(0, c(b"Commit\0"), c(b"CMT\0"));
    let p = b.machine_add_page(m, c(b"Commit\0"), c(b"CMT\0"));
    b.page_add_param(p, c(b"Tone\0"), c(b"TONE\0"), 64);
    let mode = b.page_add_param(p, c(b"Mode\0"), c(b"MODE\0"), 0);
    b.param_range(mode, 0, 3);
    b.param_latched(mode);
    let go = b.page_add_param(p, c(b"Go\0"), c(b"GO\0"), 0);
    b.param_on_click(go, 0x2000, 0);
    assert_eq!(b.err, 0);
    b
}

/// The lines of the golden file that belong to this service: the case headers, the machines' records, the voice and the args lines.
fn mine(line: &str) -> bool {
    line.starts_with("case ")
        || line.starts_with("voice ")
        || line.starts_with("args ")
        || (line.starts_with("record ") && !line.starts_with("record 4 "))
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
        panic!("the lowered records differ from {} at the machines' line {}:\n  now:      {}\n  recorded: {}", path.display(), at + 1, have.get(at).unwrap_or(&"(end)"), want.get(at).unwrap_or(&"(end)"));
    }
}

#[test]
fn nothing_of_the_rows_is_declared_before_the_declare_pass_and_a_refused_block_leaves_them_as_they_were(
) {
    use dkrt_abi::core_api::CoreApi;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = FakeCore::new();
    core.set_dynreg(dynreg(&DN2));
    // a pinned number is a run-time type of the device's profile
    core.device = dkrt_abi::profiles::DN2.id;
    // the module DSP part claimed a voice row for the pinned machine type 0x12, which has a note_on routine
    const PINNED: u32 = 0x12;
    dkrt_svc_machines::rows::reset();
    {
        let d = dkrt_svc_machines::rows::vrows();
        d.row_owner[0] = 1;
        d.row_type[0] = PINNED;
        d.row_note_on[0] = 1;
        d.row_rng[0] = [0; 8];
        d.row_latched[0] = 0;
    }
    unsafe {
        build::begin(0);
        core.running.set(1);
        core.source.set(1);
        let m = build::add_machine(&core, 8 as *mut u8, 0, c(b"Analog\0"), c(b"ANLG\0"));
        let p = build::machine_add_page(&core, m, c(b"Analog\0"), c(b"S\0"));
        let q = build::page_add_param(&core, p, c(b"Decay\0"), c(b"DECAY\0"), 80);
        build::param_range(&core, q, 0, 100);
        build::param_latched(&core, q);
    }
    let k = keys(&[PINNED], (1, 1, 1));
    let mut measured = RecordSink::measure();
    assert_eq!(commit(&core, commit_phase::PREPARE, &k, &mut measured), OK);
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    assert_eq!(commit(&core, commit_phase::WRITE, &k, &mut out), OK);
    // the loader's walker refuses the block here: it never gives the declare pass, and the rows are as they were
    assert_eq!(
        (
            dkrt_svc_machines::rows::vrows().row_rng[0],
            dkrt_svc_machines::rows::vrows().row_latched[0],
        ),
        ([0; 8], 0)
    );
    unsafe { <Machines as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!(
        (
            dkrt_svc_machines::rows::vrows().row_rng[0],
            dkrt_svc_machines::rows::vrows().row_latched[0],
        ),
        ([0; 8], 0)
    );
    // a block that passes: the declare pass puts the ranges and the latch in the row
    unsafe {
        build::begin(0);
        let m = build::add_machine(&core, 8 as *mut u8, 0, c(b"Analog\0"), c(b"ANLG\0"));
        let p = build::machine_add_page(&core, m, c(b"Analog\0"), c(b"S\0"));
        let q = build::page_add_param(&core, p, c(b"Decay\0"), c(b"DECAY\0"), 80);
        build::param_range(&core, q, 0, 100);
        build::param_latched(&core, q);
    }
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    assert_eq!(commit(&core, commit_phase::WRITE, &k, &mut out), OK);
    assert_eq!(commit(&core, commit_phase::DECLARE, &k, &mut out), OK);
    assert_eq!(dkrt_svc_machines::rows::vrows().row_latched[0], 1);
    assert_eq!(
        dkrt_svc_machines::rows::vrows().row_rng[0][1],
        100 * 256 << 16
    );
}
