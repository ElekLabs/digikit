//! Host tests of what the machines service owns of the DSP diagnostics and the table checks: the machine guard's words (`DspPart::diag`), the check of the voice table
//! against the module records (`Describe::table_check`), the fault site (`DspPart::attribute`), the state bytes of a DSP section (`dsp_check_sections`) and what a reset
//! of the service's statics clears.
use dkrt_abi::dsp::{
    DspSec, Job, MAX_ROWS, PLACEHOLDER_OWNER, VOICE_NOTE_ON, VOICE_STATE_BYTES, VOICE_V3,
    VOICE_VOICES,
};
use dkrt_abi::provider::{Describe, DspPart};
use dkrt_abi::service::TableFault;
use dkrt_sdk::fault::{DIAG_WORDS, SITE_FX_MASTER, SITE_VOICE};
use dkrt_sdk::logk;
use dkrt_svc_machines::machines::Machines;
use dkrt_svc_machines::rows::{self, vrows};
use dkrt_test_support::contract::locked;
use dkrt_test_support::{Call, FakeCore};
use std::sync::MutexGuard;

/// The module record (index) that owns voice row 1 in these tests.
const REC: usize = 3;

/// A core, the service's statics at the start (rows and guard memory), and voice row 1 owned by module record `REC` with machine type 17.
fn core() -> (MutexGuard<'static, ()>, FakeCore) {
    let lock = locked();
    rows::reset();
    let mut core = FakeCore::new();
    core.device = dkrt_abi::profiles::DT2.id;
    core.set_dyndsp(0x10000, 2, 4);
    let v = vrows();
    v.row_owner[1] = REC as u8 + 1;
    v.row_type[1] = 17;
    v.mod_rows[REC] = 1 << 1;
    (lock, core)
}

fn diag(core: &FakeCore, words: &[u32; DIAG_WORDS]) {
    DspPart::<FakeCore>::diag(&mut Machines::new(), core, words)
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

/// The guard's words: faults `n` (16 bits) of kind `kind`, `disabled` types switched off (word 17 bits 24 and up), the type of the last fault (18), its three argument
/// words and the parameter words (19 to 22), and the type switched off last (23).
fn words(n: u32, kind: u32, disabled: u32, ty: u32, off_ty: u32) -> [u32; DIAG_WORDS] {
    let mut w = [0u32; DIAG_WORDS];
    w[17] = n | kind << 16 | disabled << 24;
    w[18] = ty;
    w[19] = 0xa;
    w[20] = 0xb;
    w[21] = 0xc;
    w[23] = off_ty;
    w
}

#[test]
fn a_new_fault_logs_once_with_the_type_and_a_second_equal_block_logs_nothing() {
    let (_g, core) = core();
    let w = words(1, 2, 0, 17, 0);
    diag(&core, &w);
    assert_eq!(
        logs(&core),
        [(
            logk::MACHINE_FAULT,
            logk::F_ERROR,
            [17, 2 | 1 << 8, 0xa, 0xb, 0xc]
        )]
    );
    diag(&core, &w);
    assert_eq!(logs(&core).len(), 1, "the same block is not logged again");
    // a burst between two polls is one event: the count says how many, and the type's count grows by the difference
    diag(&core, &words(5, 2, 0, 17, 0));
    assert_eq!(
        logs(&core)[1],
        (
            logk::MACHINE_FAULT,
            logk::F_ERROR,
            [17, 2 | 5 << 8, 0xa, 0xb, 0xc]
        )
    );
    assert_eq!(dkrt_svc_machines::guard::faults(17), 5);
    assert_eq!(dkrt_svc_machines::guard::faults(18), 0);
    assert!(!dkrt_svc_machines::guard::disabled(17));
}

#[test]
fn a_type_the_guard_switches_off_logs_machine_disabled_once_and_is_marked() {
    let (_g, core) = core();
    diag(&core, &words(8, 1, 1, 17, 17));
    let l = logs(&core);
    assert_eq!(l.len(), 2);
    assert_eq!(
        l[1],
        (logk::MACHINE_DISABLED, logk::F_ERROR, [17, 1, 8, 0, 0])
    );
    assert!(dkrt_svc_machines::guard::disabled(17));
    assert!(!dkrt_svc_machines::guard::disabled(16));
    diag(&core, &words(8, 1, 1, 17, 17));
    assert_eq!(logs(&core).len(), 2, "nothing new");
    // the DSP restarted: a count that changed is an event, to 0 too (as the runtime's own guard logging was)
    diag(&core, &[0u32; DIAG_WORDS]);
    assert_eq!(logs(&core).len(), 4);
    diag(&core, &[0u32; DIAG_WORDS]);
    assert_eq!(logs(&core).len(), 4);
}

#[test]
fn a_type_past_the_counted_range_logs_but_is_counted_for_no_type() {
    let (_g, core) = core();
    diag(&core, &words(1, 0, 1, 40, 40));
    assert_eq!(logs(&core).len(), 2);
    assert_eq!(dkrt_svc_machines::guard::faults(40), 0);
    assert!(!dkrt_svc_machines::guard::disabled(40));
}

#[test]
fn the_effect_words_of_the_block_are_not_the_machine_guards() {
    let (_g, core) = core();
    let mut w = [0u32; DIAG_WORDS];
    for i in 24..DIAG_WORDS {
        w[i] = 0x1234_5678;
    }
    diag(&core, &w);
    assert!(logs(&core).is_empty());
}

#[test]
fn the_voice_site_names_the_module_of_the_machine_type_and_no_other_site_is_the_machines() {
    let (_g, core) = core();
    let at =
        |site: u32, id: u32| DspPart::<FakeCore>::attribute(&mut Machines::new(), &core, site, id);
    assert_eq!(at(SITE_VOICE, 17), Some(REC));
    assert_eq!(at(SITE_VOICE, 18), None);
    assert_eq!(at(SITE_FX_MASTER, 17), None);
    // a placeholder is no module's row
    vrows().row_owner[2] = PLACEHOLDER_OWNER;
    vrows().row_type[2] = 18;
    assert_eq!(at(SITE_VOICE, 18), None);
}

fn check(core: &FakeCore, used: u32) -> (u32, TableFault) {
    let mut f = TableFault::default();
    let r = Describe::<FakeCore>::table_check(&Machines::new(), core, used, &mut f);
    (r, f)
}

#[test]
fn the_table_check_accepts_a_consistent_table_and_a_placeholder_and_names_each_kind_of_mismatch() {
    let (_g, core) = core();
    let used = 1 << REC;
    assert_eq!(check(&core, used).0, 0);
    // a placeholder has no record and no bit
    vrows().row_owner[2] = PLACEHOLDER_OWNER;
    vrows().row_type[2] = 18;
    assert_eq!(check(&core, used).0, 0);
    // an owned row of a record that is not used
    let (r, f) = check(&core, 0);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 1,
            b: REC as u32 + 1
        }
    );
    // an owned row whose record has no bit for it
    vrows().mod_rows[REC] = 0;
    let (r, f) = check(&core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 1,
            b: REC as u32 + 1
        }
    );
    // a bit for a row that nobody owns
    vrows().mod_rows[REC] = 1 << 1 | 1 << 3;
    let (r, f) = check(&core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 3,
            b: 0
        }
    );
    // a bit past the table
    vrows().mod_rows[REC] = 1 << 1 | 1 << 20;
    let (r, f) = check(&core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: REC as u32,
            a: 20,
            b: 0xff
        }
    );
    // a record that is not used holds rows
    vrows().mod_rows[REC] = 1 << 1;
    vrows().mod_rows[5] = 1 << 3;
    assert_ne!(check(&core, used).0, 0);
    // an owner past the records
    vrows().mod_rows[5] = 0;
    vrows().row_owner[3] = 200;
    let (r, f) = check(&core, used);
    assert_ne!(r, 0);
    assert_eq!(
        f,
        TableFault {
            module: 0,
            a: 3,
            b: 200
        }
    );
    assert!(MAX_ROWS > 3);
}

/// A version 3 DSP section with `n` voices, each with `voices` states of `state` bytes, over a header that holds them.
fn section(n: u32, voices: u32, state: u32) -> (Vec<u8>, DspSec) {
    let image_size = 0x100u32;
    let voice_off = 0x40u32;
    let image_off = voice_off + n * VOICE_V3;
    let mut s = vec![0u8; (image_off + image_size) as usize];
    for i in 0..n {
        let e = (voice_off + VOICE_V3 * i) as usize;
        let mut put = |at: u32, v: u32| s[e + at as usize..][..4].copy_from_slice(&v.to_be_bytes());
        put(VOICE_NOTE_ON, 0x10);
        put(VOICE_STATE_BYTES, state);
        put(VOICE_VOICES, voices);
    }
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
        voice_off,
        nvoice: n,
        fx_off: 0,
        nfx: 0,
        vstride: VOICE_V3,
        bank_off: 0,
    };
    (s, sec)
}

fn check_sections(core: &FakeCore, sec: &DspSec, job: &mut Job) -> i32 {
    match DspPart::<FakeCore>::check_sections(&mut Machines::new(), core, sec, job) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

#[test]
fn the_section_check_adds_the_state_blocks_of_the_voices_rounded_to_eight_and_counts_the_entries() {
    let (_g, core) = core();
    let (_keep, sec) = section(2, 4, 20);
    let mut job = Job::empty();
    assert_eq!(check_sections(&core, &sec, &mut job), dkrt_sdk::OK);
    assert_eq!(
        job.state_bytes,
        2 * 4 * 24,
        "each of the two voices has 4 states of 20 bytes, each taking 24"
    );
    assert_eq!(job.accepted, 2);
    // a state larger than the area is refused
    let (_keep, big) = section(1, 1, 0x10004);
    assert_eq!(
        check_sections(&core, &big, &mut Job::empty()),
        dkrt_sdk::E_DSP_FORMAT
    );
    // a table that does not fit the section, or more voices than rows
    let (_keep, mut out) = section(2, 1, 8);
    out.voice_off = out.size - VOICE_V3;
    assert_eq!(
        check_sections(&core, &out, &mut Job::empty()),
        dkrt_sdk::E_DSP_FORMAT
    );
    let (_keep, mut many) = section(1, 1, 8);
    many.nvoice = MAX_ROWS as u32 + 1;
    assert_eq!(
        check_sections(&core, &many, &mut Job::empty()),
        dkrt_sdk::E_DSP_FORMAT
    );
    // a section without voices adds nothing
    let (_keep, none) = section(0, 0, 0);
    let mut job = Job::empty();
    assert_eq!(check_sections(&core, &none, &mut job), dkrt_sdk::OK);
    assert_eq!((job.state_bytes, job.accepted), (0, 0));
}

#[test]
fn a_reset_of_the_service_clears_the_rows_and_the_guard_memory() {
    let (_g, core) = core();
    diag(&core, &words(3, 1, 1, 17, 17));
    assert!(dkrt_svc_machines::guard::disabled(17));
    rows::reset();
    assert!(!dkrt_svc_machines::guard::disabled(17));
    assert_eq!(dkrt_svc_machines::guard::faults(17), 0);
    // the guard has seen nothing again: the same block logs again
    let before = logs(&core).len();
    diag(&core, &words(3, 1, 1, 17, 17));
    assert_eq!(logs(&core).len(), before + 2);
    // the rows start free again (the accessor sets the free type)
    assert_eq!(vrows().row_owner[1], 0);
    assert_eq!(vrows().row_type[1], dkrt_abi::dsp::FREE_TYPE);
}

#[test]
fn without_the_table_of_the_dsp_the_service_has_no_voice_rows_and_the_table_still_checks() {
    let lock = locked();
    rows::reset();
    let core = FakeCore::new();
    unsafe {
        assert!(dkrt_svc_machines::dsp::dyndsp(&core).is_null());
        assert_eq!(dkrt_svc_machines::dsp::rows_counts(&core), (0, 0));
    }
    assert_eq!(check(&core, 0).0, 0);
    // a table of another kind of image (no magic) is no table
    let mut other = FakeCore::new();
    other
        .info_tables
        .push((dkneeds::keys::name_hash("dyndsp_info"), vec![0u32; 16]));
    unsafe { assert!(dkrt_svc_machines::dsp::dyndsp(&other).is_null()) };
    drop(lock);
}
