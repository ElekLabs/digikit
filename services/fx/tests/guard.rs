//! Host tests of what the fx service owns of the DSP diagnostics and the table checks: the effect guard's words (`DspPart::diag`), the check of the effect table
//! against the module records (`Describe::table_check`), the fault sites (`DspPart::attribute`) and the state bytes of a DSP section (`dsp_check_sections`).
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job, FX_DESC_BYTES, FX_DESC_MAGIC, FX_REC, FX_ROW_WORDS};
use dkrt_abi::provider::{Describe, DspPart};
use dkrt_abi::service::TableFault;
use dkrt_sdk::fault::{DIAG_WORDS, SITE_FX_MASTER, SITE_FX_SEND, SITE_VOICE};
use dkrt_sdk::logk;
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::rows::{self, frows};
use dkrt_test_support::contract::locked;
use dkrt_test_support::{Call, FakeCore};
use std::sync::MutexGuard;

const ROW_BYTES: u32 = 4 * FX_ROW_WORDS;
const FIRST_ROW: u32 = 6;
/// The module record (index) that owns effect row 1 of the service's table in these tests.
const REC: usize = 3;

/// A core with the effect table starting at row `FIRST_ROW`, the service's rows at the start, an owner whose guard has seen nothing, and effect row 1 owned by module record `REC`.
fn core() -> (MutexGuard<'static, ()>, FakeCore, Fx) {
    let rows_lock = locked();
    rows::reset();
    let mut core = FakeCore::new();
    core.set_dynfx(FIRST_ROW, 4);
    frows().fx_owner[1] = REC as u8 + 1;
    frows().mod_fx[REC] = 1 << 1;
    (rows_lock, core, Fx::new())
}

fn diag(fx: &mut Fx, core: &FakeCore, words: &[u32; DIAG_WORDS]) {
    DspPart::diag(fx, core, words)
}

fn logs(core: &FakeCore) -> Vec<(u16, u16, [u32; 5])> {
    core.calls()
        .iter()
        .filter_map(|c| match c {
            Call::Log { kind, flags, args } => Some((*kind, *flags, *args)),
            _ => None,
        })
        .collect()
}

/// The words of a reply: effect faults `n` with the last fault at byte offset `off` of the table (word 28), and the row switched off last (word 31, offset + 1).
fn words(n: u32, off: u32, disabled: Option<u32>) -> [u32; DIAG_WORDS] {
    let mut w = [0u32; DIAG_WORDS];
    w[28] = n | (off + 1) << 16;
    w[31] = disabled.map_or(0, |o| o + 1);
    w
}

/// The source tag of the event of module record `REC`: its index + 1 in the source bits.
fn source() -> u16 {
    (REC as u16 + 1) << logk::SOURCE_SHIFT
}

#[test]
fn a_new_effect_fault_logs_once_with_the_module_of_the_row_and_a_second_equal_block_logs_nothing() {
    let (_rows, core, mut fx) = core();
    let row = FIRST_ROW + 1;
    let w = words(1, row * ROW_BYTES, None);
    diag(&mut fx, &core, &w);
    assert_eq!(
        logs(&core),
        [(logk::FX_FAULT, logk::F_ERROR | source(), [row, 1, 0, 0, 0])]
    );
    diag(&mut fx, &core, &w);
    assert_eq!(logs(&core).len(), 1, "the same block is not logged again");
    // a burst between two polls is one event: the count says how many
    diag(&mut fx, &core, &words(5, row * ROW_BYTES, None));
    assert_eq!(
        logs(&core)[1],
        (logk::FX_FAULT, logk::F_ERROR | source(), [row, 5, 0, 0, 0])
    );
    // a row of no module has no source
    let other = FIRST_ROW + 2;
    diag(&mut fx, &core, &words(6, other * ROW_BYTES, None));
    assert_eq!(
        logs(&core)[2],
        (logk::FX_FAULT, logk::F_ERROR, [other, 6, 0, 0, 0])
    );
}

#[test]
fn a_row_switched_off_logs_once_and_a_cleared_word_logs_nothing() {
    let (_rows, core, mut fx) = core();
    let row = FIRST_ROW + 1;
    diag(
        &mut fx,
        &core,
        &words(8, row * ROW_BYTES, Some(row * ROW_BYTES)),
    );
    let l = logs(&core);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].0, logk::FX_FAULT);
    assert_eq!(
        l[1],
        (
            logk::FX_DISABLED,
            logk::F_ERROR | source(),
            [row, 8, 0, 0, 0]
        )
    );
    diag(
        &mut fx,
        &core,
        &words(8, row * ROW_BYTES, Some(row * ROW_BYTES)),
    );
    assert_eq!(logs(&core).len(), 2);
    // the word back to 0 (the DSP restarted): remembered, no event
    diag(&mut fx, &core, &words(8, row * ROW_BYTES, None));
    assert_eq!(logs(&core).len(), 2);
}

#[test]
fn the_machines_words_of_the_block_are_not_the_effect_guards() {
    let (_rows, core, mut fx) = core();
    let mut w = [0u32; DIAG_WORDS];
    for i in 14..24 {
        w[i] = 0x1234_5678;
    }
    diag(&mut fx, &core, &w);
    assert!(logs(&core).is_empty());
}

#[test]
fn fault_sites_are_named_by_the_byte_offset_and_the_voice_site_is_not_the_effect_services() {
    let (_rows, core, mut fx) = core();
    let mut at = |site: u32, id: u32| DspPart::attribute(&mut fx, &core, site, id);
    let off = (FIRST_ROW + 1) * ROW_BYTES;
    assert_eq!(at(SITE_FX_MASTER, off), Some(REC));
    assert_eq!(at(SITE_FX_SEND, off), Some(REC));
    // a byte inside the row is that row
    assert_eq!(at(SITE_FX_MASTER, off + ROW_BYTES - 4), Some(REC));
    assert_eq!(
        at(SITE_FX_MASTER, off + ROW_BYTES),
        None,
        "the next row is free"
    );
    assert_eq!(
        at(SITE_FX_MASTER, 0),
        None,
        "a row before the first free one"
    );
    assert_eq!(at(SITE_VOICE, off), None);
}

fn check(fx: &Fx, core: &FakeCore, used: u32) -> (u32, TableFault) {
    let mut f = TableFault::default();
    let r = Describe::table_check(fx, core, used, &mut f);
    (r, f)
}

#[test]
fn the_table_check_accepts_a_consistent_table_and_names_each_kind_of_mismatch() {
    let (_rows, core, fx) = core();
    let used = 1 << REC;
    assert_eq!(check(&fx, &core, used).0, 0);
    // an owned row of a record that is not used
    let (r, f) = check(&fx, &core, 0);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 0x401,
            b: REC as u32 + 1
        }
    );
    // an owned row whose record has no bit for it
    frows().mod_fx[REC] = 0;
    let (r, f) = check(&fx, &core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 0x401,
            b: REC as u32 + 1
        }
    );
    // a bit for a row that nobody owns
    frows().mod_fx[REC] = 1 << 1 | 1 << 2;
    let (r, f) = check(&fx, &core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 0x402,
            b: 0
        }
    );
    // a bit past the table
    frows().mod_fx[REC] = 1 << 1 | 1 << 20;
    let (r, f) = check(&fx, &core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 0x400 | 20,
            b: 0xff
        }
    );
    // a record that is not used holds rows
    frows().mod_fx[REC] = 1 << 1;
    frows().mod_fx[5] = 1 << 3;
    assert_ne!(check(&fx, &core, used).0, 0);
    // an owner past the records
    frows().mod_fx[5] = 0;
    frows().fx_owner[2] = 200;
    let (r, f) = check(&fx, &core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: 0,
            a: 0x402,
            b: 200
        }
    );
}

/// A DSP section with `n` effect records, each with `state` bytes of state, over a header that holds them.
fn section(n: u32, state: u32) -> (Vec<u8>, DspSec) {
    let image_size = 0x100u32;
    let fx_off = 0x40u32;
    let image_off = fx_off + n * FX_REC;
    let mut s = vec![0u8; (image_off + image_size) as usize];
    for i in 0..n {
        let e = (fx_off + FX_REC * i) as usize;
        let mut put = |at: u32, v: u32| s[e + at as usize..][..4].copy_from_slice(&v.to_be_bytes());
        put(dkrt_abi::dsp::FX_PROCESS, 0x10);
        put(dkrt_abi::dsp::FX_DESC, 0x20);
        put(dkrt_abi::dsp::FX_STATE, state);
    }
    // the descriptor at image offset 0x20 (little endian words)
    let d = (image_off + 0x20) as usize;
    let words = [FX_DESC_MAGIC, dkneeds::fx::DK_FX_V1, 0, state];
    for (k, at) in [
        dkneeds::fx::DK_FX_DESC_MAGIC,
        dkneeds::fx::DK_FX_DESC_CONTRACT,
        dkneeds::fx::DK_FX_DESC_N_PARAMS,
        dkneeds::fx::DK_FX_DESC_STATE_BYTES,
    ]
    .iter()
    .enumerate()
    {
        s[d + *at as usize..][..4].copy_from_slice(&words[k].to_le_bytes());
    }
    assert!(FX_DESC_BYTES as usize <= image_size as usize - 0x20);
    let sec = DspSec {
        s: s.as_ptr(),
        version: 5,
        size: s.len() as u32,
        image_size,
        image_off,
        reloc_off: 0,
        nreloc: 0,
        import_off: 0,
        nimport: 0,
        voice_off: 0,
        nvoice: 0,
        fx_off,
        nfx: n,
        vstride: dkrt_abi::dsp::VOICE_V3,
        bank_off: 0,
    };
    (s, sec)
}

fn check_sections(fx: &mut Fx, core: &FakeCore, sec: &DspSec, job: &mut Job) -> Result<(), i32> {
    DspPart::check_sections(fx, core, sec, job)
}

#[test]
fn the_section_check_adds_the_states_of_the_effects_rounded_to_eight() {
    let (_rows, mut core, mut fx) = core();
    // the area of the DSP: the state of an effect must fit it
    core.set_dyndsp(0x10000, 0, 0);
    let (_keep, sec) = section(3, 20);
    let mut job = Job::empty();
    assert_eq!(check_sections(&mut fx, &core, &sec, &mut job), Ok(()));
    assert_eq!(job.state_bytes, 3 * 24, "each state of 20 bytes takes 24");
    assert_eq!(
        job.accepted, 3,
        "the service says it took the three entries"
    );
    // a state larger than the area is refused
    let (_keep, big) = section(1, 0x10004);
    assert_eq!(
        check_sections(&mut fx, &core, &big, &mut Job::empty()),
        Err(dkrt_sdk::E_DSP_FORMAT)
    );
    // more effects than the chain has rows
    let (_keep, many) = section(dkrt_abi::dsp::MAX_FX as u32 + 1, 8);
    assert_eq!(
        check_sections(&mut fx, &core, &many, &mut Job::empty()),
        Err(dkrt_sdk::E_DSP_FORMAT)
    );
    // a table that does not lie inside the section
    let (_keep, mut out) = section(2, 8);
    out.fx_off = out.size - FX_REC;
    assert_eq!(
        check_sections(&mut fx, &core, &out, &mut Job::empty()),
        Err(dkrt_sdk::E_DSP_FORMAT)
    );
    // a section without effects adds nothing
    let (_keep, none) = section(0, 0);
    let mut job = Job::empty();
    assert_eq!(check_sections(&mut fx, &core, &none, &mut job), Ok(()));
    assert_eq!(job.state_bytes, 0);
}

#[test]
fn without_the_table_of_the_effect_chain_the_service_has_no_rows_and_no_slot() {
    let _rows = locked();
    rows::reset();
    let core = FakeCore::new();
    unsafe {
        assert_eq!(dkrt_svc_fx::dsp::fx_free_rows(&core), 0);
        assert!(dkrt_svc_fx::dsp::fx_slot(&core, 0, 0).is_none());
        assert!(dkrt_svc_fx::dsp::dynfx(&core).is_null());
    }
    // a table of another device, or of another row size, is not the table of this runtime's effect chain
    let mut core = FakeCore::new();
    core.device = 1;
    core.set_dynfx(0, 4);
    core.device = 2;
    unsafe { assert!(dkrt_svc_fx::dsp::dynfx(&core).is_null()) };
    core.device = 1;
    unsafe { assert!(!dkrt_svc_fx::dsp::dynfx(&core).is_null()) };
    let hash = dkneeds::keys::name_hash("dynfx_info");
    core.info_tables
        .iter_mut()
        .find(|(h, _)| *h == hash)
        .unwrap()
        .1[dkrt_abi::dsp::F_WORDS] = 20;
    unsafe { assert!(dkrt_svc_fx::dsp::dynfx(&core).is_null()) };
    let _ = core.device_id();
}
