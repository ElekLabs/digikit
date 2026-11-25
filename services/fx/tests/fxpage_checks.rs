//! Host tests of the effect page checks over `FakeCore`: which EFFECT records `fxpage::check_record` accepts and which device facts `fxpage::bind_facts` binds.
//! Both read no firmware address: the record is a byte array and the facts are constants. The tests share the bound table of the module
//! (`DKRT_FXP_TAB`), so each takes the lock and unbinds before and after.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_sdk::{E_FORMAT, OK};
use dkrt_svc_fx::fxpage;
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

/// Binds the table in a test and gives it back at the end, also when the test fails.
struct Bound<'a>(std::sync::MutexGuard<'a, ()>);

fn lock() -> Bound<'static> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { fxpage::unbind() };
    Bound(g)
}

impl Drop for Bound<'_> {
    fn drop(&mut self) {
        unsafe { fxpage::unbind() };
    }
}

/// The facts of a device with the chain's rows and the kit's 32 values.
fn facts() -> fxpage::DeviceFacts {
    fxpage::DeviceFacts {
        device: FakeCore::new().device_id(),
        slots: MAX_FX as u32,
        values_n: 32,
        knob_block: 2,
        send_block: 3,
        ..Default::default()
    }
}

/// An image whose `rows_info` holds the blocks of the effect knobs and the send knobs.
fn core_for() -> FakeCore {
    let mut core = FakeCore::new();
    core.set_rows_blocks(&[(2, 40, 8 * MAX_FX as u32, 0x75), (3, 90, 2, 0x75)]);
    core
}

fn put32(r: &mut [u8], at: usize, v: u32) {
    r[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn put_f(r: &mut [u8], at: usize, v: f32) {
    put32(r, at, v.to_bits());
}

/// `s` as a NUL padded text of `field` bytes at `at`.
fn text(r: &mut [u8], at: usize, field: usize, s: &[u8]) {
    r[at..at + field].fill(0);
    r[at..at + s.len()].copy_from_slice(s);
}

const KNOB_AT: usize = 48;
const KNOB_REC: usize = 52;

fn knob(
    r: &mut [u8],
    j: usize,
    pos: u32,
    disp: u32,
    lo: f32,
    hi: f32,
    def: f32,
    name: &[u8],
    short: &[u8],
) {
    let k = KNOB_AT + j * KNOB_REC;
    put32(r, k, pos);
    put32(r, k + 4, disp);
    put_f(r, k + 8, lo);
    put_f(r, k + 12, hi);
    put_f(r, k + 16, def);
    text(r, k + 20, 24, name);
    text(r, k + 44, 8, short);
}

/// An effect record of row 1 with the id 7, the tab REV and two knobs (one of them bipolar).
fn good() -> Vec<u8> {
    let mut r = vec![0u8; fxpage::LEN_EFFECT as usize];
    put32(&mut r, 0, 1);
    put32(&mut r, 4, 64);
    put32(&mut r, 8, 2);
    put32(&mut r, 12, 7);
    text(&mut r, 16, 8, b"REV");
    text(&mut r, 24, 24, b"Reverb");
    knob(&mut r, 0, 0, 0, 0.0, 1.0, 0.5, b"Size", b"SIZE");
    knob(&mut r, 1, 3, 1, -1.0, 1.0, 0.0, b"Tone", b"TONE");
    r
}

fn check(r: &[u8]) -> i32 {
    let f = facts();
    let core = core_for();
    unsafe {
        assert_eq!(fxpage::bind_facts(&core, &f, [0, 0]), OK);
        fxpage::check_record(&core, r.as_ptr())
    }
}

#[test]
fn a_well_formed_effect_record_is_accepted_and_a_knob_past_the_count_is_not_looked_at() {
    let _g = lock();
    let mut r = good();
    assert_eq!(check(&r), OK);
    // knob 5 of 2: garbage there is not read
    let k = KNOB_AT + 5 * KNOB_REC;
    r[k..k + KNOB_REC].fill(0xff);
    assert_eq!(check(&r), OK);
    // no knob at all is a page with MIX only
    put32(&mut r, 8, 0);
    assert_eq!(check(&r), OK);
}

#[test]
fn an_effect_record_with_a_bad_row_mix_count_or_id_is_refused() {
    let _g = lock();
    for (at, v, what) in [
        (0, MAX_FX as u32, "row past the chain"),
        (4, 128, "mix over 127"),
        (8, 8, "more knobs than the page has"),
        (12, 0, "id 0"),
        (12, 0x1_0000, "id over 16 bits"),
    ] {
        let mut r = good();
        put32(&mut r, at, v);
        assert_eq!(check(&r), E_FORMAT, "{what}");
    }
    // the last values that are fine
    let mut r = good();
    put32(&mut r, 0, MAX_FX as u32 - 1);
    put32(&mut r, 4, 127);
    put32(&mut r, 8, 7);
    put32(&mut r, 12, 0xffff);
    for (j, pos) in [1u32, 2, 4, 5, 6].into_iter().enumerate() {
        knob(&mut r, j + 2, pos, 0, 0.0, 1.0, 0.5, b"K", b"K");
    }
    assert_eq!(check(&r), OK);
}

/// The good record with `edit` applied.
fn edited(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut r = good();
    edit(&mut r);
    r
}

#[test]
fn the_texts_of_an_effect_record_are_printable_and_fit_their_fields() {
    let _g = lock();
    for (what, r) in [
        ("empty tab", edited(|r| text(r, 16, 8, b""))),
        ("tab of 4 characters", edited(|r| text(r, 16, 8, b"ABCD"))),
        (
            "a control character in the tab",
            edited(|r| text(r, 16, 8, b"A\x1fB")),
        ),
        ("bytes after the NUL of the tab", edited(|r| r[19] = b'Z')),
        ("empty title", edited(|r| text(r, 24, 24, b""))),
        ("a title with no NUL", edited(|r| r[24..48].fill(b'x'))),
        (
            "empty knob name",
            edited(|r| text(r, KNOB_AT + 20, 24, b"")),
        ),
        (
            "knob short name of 7 characters",
            edited(|r| text(r, KNOB_AT + 44, 8, b"SEVENCH")),
        ),
        (
            "a high byte in the knob name",
            edited(|r| r[KNOB_AT + 20] = 0x80),
        ),
    ] {
        assert_eq!(check(&r), E_FORMAT, "{what}");
    }
    // 3 characters of the tab, 6 of a short name and 23 of a title are the longest
    let mut r = good();
    text(&mut r, 24, 24, &[b'T'; 23]);
    text(&mut r, KNOB_AT + 44, 8, b"SIXCHR");
    assert_eq!(check(&r), OK);
}

#[test]
fn a_knob_of_an_effect_record_has_its_own_position_a_range_and_a_default_inside_it() {
    let _g = lock();
    let first =
        |pos, disp, lo, hi, def| edited(|r| knob(r, 0, pos, disp, lo, hi, def, b"Size", b"SIZE"));
    for (what, r) in [
        ("position 8", first(8, 0, 0.0, 1.0, 0.5)),
        (
            "the same position twice",
            edited(|r| knob(r, 1, 0, 0, 0.0, 1.0, 0.5, b"Tone", b"TONE")),
        ),
        ("display kind 2", first(0, 2, 0.0, 1.0, 0.5)),
        ("min equal to max", first(0, 0, 1.0, 1.0, 1.0)),
        ("min over max", first(0, 0, 1.0, 0.0, 0.5)),
        ("default under min", first(0, 0, 0.0, 1.0, -0.1)),
        ("default over max", first(0, 0, 0.0, 1.0, 1.1)),
        ("a NaN min", first(0, 0, f32::NAN, 1.0, 0.5)),
        ("a NaN default", first(0, 0, 0.0, 1.0, f32::NAN)),
        ("an infinite max", first(0, 0, 0.0, f32::INFINITY, 0.5)),
        ("a span that overflows", first(0, 0, -3e38, 3e38, 0.0)),
    ] {
        assert_eq!(check(&r), E_FORMAT, "{what}");
    }
    // the ends of the range are inside it
    let mut r = good();
    knob(&mut r, 0, 7, 0, 0.0, 1.0, 1.0, b"Size", b"SIZE");
    knob(&mut r, 1, 0, 1, -1.0, 1.0, -1.0, b"Tone", b"TONE");
    assert_eq!(check(&r), OK);
}

#[test]
fn facts_are_bound_only_when_the_sizes_fit_what_the_service_holds_state_for() {
    let _g = lock();
    let bound = |f: fxpage::DeviceFacts| {
        let core = core_for();
        let r = unsafe { fxpage::bind_facts(&core, &f, [0, 0]) } == OK;
        unsafe { fxpage::unbind() };
        r
    };
    assert!(bound(facts()));
    for (what, f) in [
        (
            "more pages than the chain has rows",
            fxpage::DeviceFacts {
                slots: MAX_FX as u32 + 1,
                ..facts()
            },
        ),
        (
            "more values than value indices",
            fxpage::DeviceFacts {
                values_n: 33,
                ..facts()
            },
        ),
    ] {
        assert!(!bound(f), "{what}");
    }
    // no `rows_info` in the image, or none for another device: nothing to bind
    let core = FakeCore::new();
    assert!(!unsafe { fxpage::available(&core) });
}
