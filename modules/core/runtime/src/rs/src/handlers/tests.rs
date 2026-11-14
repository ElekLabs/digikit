//! `FakeService`: a `Service` that records what the core calls (record kind, owner, bytes of `check`, `apply` and `remove`; the
//! declarations, attributions and `on_loaded`), and host tests of the core's walkers over it (docs/ARCHITECTURE.md "Boundaries and tests").
use super::*;
use dkrt_sdk::*;
use crate::registry::{apply_with, check_with, each, remove_with, DKRT_REG};
use dkrt_sdk::proto::CAPK_FX_SLOTS;
use std::sync::Mutex;
use std::vec::Vec;

/// Serializes the tests that use the fake's log and the registry's state.
pub(crate) static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Check { svc: u8, step: u32, kind: u32, body: Vec<u8> },
    /// The rows a service writes before the per-record pass.
    Bulk { svc: u8, owner: u8 },
    Apply { svc: u8, owner: u8, kind: u32, body: Vec<u8> },
    Remove { svc: u8, owner: u8 },
    Attribute { svc: u8, site: u32, id: u32 },
    DeclareVoice { svc: u8, rec: usize, ty: u32 },
    OnLoaded { svc: u8, rec: usize },
    Tick { svc: u8, phase: u32 },
}

static mut LOG: Vec<Call> = Vec::new();
/// `(svc, kind)`: the service that refuses a record of that kind in `check`.
static mut REFUSE: (u8, u32) = (0, 0);
/// The service whose `apply` fails.
static mut FAIL_APPLY: (u8, u32) = (0, 0);

pub(crate) fn log() -> &'static mut Vec<Call> {
    unsafe { &mut *core::ptr::addr_of_mut!(LOG) }
}

/// The fake service number `N` owns record kind `100 + N`, a record of `8 + 4 * N` bytes.
pub(crate) struct Fake<const N: u8>;

pub(crate) const fn kind_of(n: u8) -> u32 {
    100 + n as u32
}

unsafe fn body(reg: *const u8, kind: u32, b: *const u8) -> Vec<u8> {
    let _ = (reg, kind);
    core::slice::from_raw_parts(b, 4 * (kind as usize - 100)).to_vec()
}

impl<const N: u8> Service for Fake<N> {
    fn len(kind: u32) -> Option<u32> {
        if kind == kind_of(N) {
            Some(8 + 4 * N as u32)
        } else {
            None
        }
    }

    /// Service 1 checks its records at the step of native pages, service 2 at the step of effects: the order of the old body.
    unsafe fn check(step: u32, c: &CheckCtx) -> i32 {
        let mine = if N == 1 { check_step::NATIVE } else { check_step::EFFECTS };
        let mut r = OK;
        if step != mine {
            return r;
        }
        each(c.reg, |kind, b| {
            if kind == kind_of(N) && r == OK {
                log().push(Call::Check { svc: N, step, kind, body: body(c.reg, kind, b) });
                if REFUSE == (N, kind) {
                    r = E_FORMAT;
                }
            }
        });
        r
    }

    unsafe fn apply(c: &ApplyCtx) -> i32 {
        log().push(Call::Bulk { svc: N, owner: c.owner });
        OK
    }

    unsafe fn apply_record(kind: u32, b: *const u8, c: &ApplyCtx) -> i32 {
        if kind != kind_of(N) {
            return OK;
        }
        log().push(Call::Apply { svc: N, owner: c.owner, kind, body: body(c.reg, kind, b) });
        if FAIL_APPLY == (N, kind) {
            E_FORMAT
        } else {
            OK
        }
    }

    unsafe fn remove(owner: u8) {
        log().push(Call::Remove { svc: N, owner });
    }

    unsafe fn caps(out: &mut CapSink) {
        out.add(CAPK_FX_SLOTS, 10 * N as u32, N as u32);
    }

    unsafe fn attribute(site: u32, id: u32) -> Option<usize> {
        log().push(Call::Attribute { svc: N, site, id });
        if site == N as u32 {
            Some(id as usize + 100 * N as usize)
        } else {
            None
        }
    }

    unsafe fn on_loaded(rec: usize) {
        log().push(Call::OnLoaded { svc: N, rec });
    }

    unsafe fn tick(phase: u32) {
        log().push(Call::Tick { svc: N, phase });
    }

    unsafe fn declare_voice(rec: usize, ty: u32, _: [u32; 8], _: u32) -> Option<i32> {
        log().push(Call::DeclareVoice { svc: N, rec, ty });
        if ty == N as u32 {
            Some(OK)
        } else {
            None
        }
    }
}

type Two = (Fake<1>, (Fake<2>, ()));

/// The words of a `dynreg_info` that the walker reads: the magic and the device.
static TABLE: [u32; 100] = {
    let mut t = [0u32; 100];
    t[0] = 0x4459_4e52;
    t[2] = 1;
    t
};

pub(crate) fn dynreg_table() -> *const u32 {
    TABLE.as_ptr()
}

/// A `register` section of the given records `(kind, 4 * (kind - 100) body bytes filled with the record number)`.
fn section(kinds: &[u32]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0x444b_5247u32.to_be_bytes());
    v.extend_from_slice(&1u32.to_be_bytes());
    v.extend_from_slice(&1u32.to_be_bytes()); // the device
    v.extend_from_slice(&(kinds.len() as u32).to_be_bytes());
    for (i, &k) in kinds.iter().enumerate() {
        let n = 4 * (k as usize - 100);
        v.extend_from_slice(&k.to_be_bytes());
        v.extend_from_slice(&(8 + n as u32).to_be_bytes());
        v.extend(core::iter::repeat(i as u8 + 1).take(n));
    }
    v
}

unsafe fn reset() {
    log().clear();
    REFUSE = (0, 0);
    FAIL_APPLY = (0, 0);
    (*core::ptr::addr_of_mut!(DKRT_REG)).registers = 0;
    (*core::ptr::addr_of_mut!(DKRT_REG)).removes = 0;
}

fn rec(svc: u8, i: u8) -> (u32, Vec<u8>) {
    (kind_of(svc), vec![i; 4 * svc as usize])
}

#[test]
fn a_mixed_file_checks_in_step_order_applies_in_file_order_and_removes_in_reverse() {
    let _g = LOCK.lock().unwrap();
    unsafe {
        reset();
        // mixed kinds: service 1, 2, 1
        let sec = section(&[kind_of(1), kind_of(2), kind_of(1)]);
        assert_eq!(check_with::<Two>(sec.as_ptr(), sec.len() as u32, 0), OK);
        assert_eq!(apply_with::<Two>(5, sec.as_ptr(), sec.len() as u32, 0x1000), OK);
        remove_with::<Two>(5);
        let (k1, b1) = rec(1, 1);
        let (k1b, b1b) = rec(1, 3);
        let (k2, b2) = rec(2, 2);
        let n = check_step::NATIVE;
        let e = check_step::EFFECTS;
        assert_eq!(
            *log(),
            vec![
                // check: step by step, so service 1's step (native pages) before service 2's (effects), its records in file order
                Call::Check { svc: 1, step: n, kind: k1, body: b1.clone() },
                Call::Check { svc: 1, step: n, kind: k1b, body: b1b.clone() },
                Call::Check { svc: 2, step: e, kind: k2, body: b2.clone() },
                // apply: every service's rows first, then each record in file order across the kinds
                Call::Bulk { svc: 1, owner: 5 },
                Call::Bulk { svc: 2, owner: 5 },
                Call::Apply { svc: 1, owner: 5, kind: k1, body: b1 },
                Call::Apply { svc: 2, owner: 5, kind: k2, body: b2 },
                Call::Apply { svc: 1, owner: 5, kind: k1b, body: b1b },
                // remove: the last service first
                Call::Remove { svc: 2, owner: 5 },
                Call::Remove { svc: 1, owner: 5 },
            ]
        );
        let r = &*core::ptr::addr_of!(DKRT_REG);
        assert_eq!((r.registers, r.removes), (1, 1));
    }
}

#[test]
fn the_first_fault_is_the_first_step_that_finds_one_and_a_failed_apply_stops_the_file() {
    let _g = LOCK.lock().unwrap();
    unsafe {
        reset();
        // an effect-step fault precedes nothing: the native step runs first, as in the old body, wherever the records sit in the file
        let sec = section(&[kind_of(2), kind_of(1), kind_of(2)]);
        REFUSE = (2, kind_of(2));
        assert_eq!(check_with::<Two>(sec.as_ptr(), sec.len() as u32, 0), E_FORMAT);
        assert_eq!(log().len(), 2); // service 1's record, then service 2's first (refused) record; the second is not looked at
        assert!(matches!(log()[0], Call::Check { svc: 1, .. }));
        reset();
        REFUSE = (1, kind_of(1));
        assert_eq!(check_with::<Two>(sec.as_ptr(), sec.len() as u32, 0), E_FORMAT);
        assert_eq!(log().len(), 1, "service 1 refused at its step, so service 2's step never ran");

        // apply stops at the first failing record in file order; later records of any kind are not applied
        reset();
        FAIL_APPLY = (1, kind_of(1));
        assert_eq!(apply_with::<Two>(5, sec.as_ptr(), sec.len() as u32, 0), E_FORMAT);
        let applied: Vec<_> = log().iter().filter(|c| matches!(c, Call::Apply { .. })).collect();
        assert_eq!(applied.len(), 2, "the effect record before the failing one was applied, the one after was not");
        assert_eq!((*core::ptr::addr_of!(DKRT_REG)).registers, 0, "a failed apply is not a registration");
    }
}

#[test]
fn the_walker_refuses_a_kind_no_service_declared_and_a_wrong_length() {
    let _g = LOCK.lock().unwrap();
    unsafe {
        reset();
        // kind 99 is no one's
        let mut bad = section(&[kind_of(1)]);
        bad[12..16].copy_from_slice(&2u32.to_be_bytes());
        bad.extend_from_slice(&99u32.to_be_bytes());
        bad.extend_from_slice(&8u32.to_be_bytes());
        assert_eq!(check_with::<Two>(bad.as_ptr(), bad.len() as u32, 0), E_FORMAT);
        assert!(log().is_empty());
        let mut wrong = section(&[kind_of(2)]);
        wrong[20..24].copy_from_slice(&12u32.to_be_bytes()); // the record's length word: a kind-1 length
        assert_eq!(check_with::<Two>(wrong.as_ptr(), wrong.len() as u32, 0), E_FORMAT);
        assert!(log().is_empty());
    }
}

#[test]
fn capacity_sums_the_declared_counts() {
    unsafe {
        let mut sink = CapSink::new();
        <Two as ServiceList>::caps(&mut sink);
        // 10 + 20 slots, 1 + 2 used: two services declare the same kind
        assert_eq!(sink.get(CAPK_FX_SLOTS), Some((30, 3)));
        assert_eq!(sink.get(CAPK_FX_SLOTS + 1), None);
    }
}

#[test]
fn attribution_asks_the_services_in_order_until_one_answers() {
    let _g = LOCK.lock().unwrap();
    unsafe {
        reset();
        assert_eq!(<Two as ServiceList>::attribute(2, 7), Some(207));
        assert_eq!(
            *log(),
            vec![Call::Attribute { svc: 1, site: 2, id: 7 }, Call::Attribute { svc: 2, site: 2, id: 7 }]
        );
        assert_eq!(<Two as ServiceList>::attribute(9, 7), None);
        reset();
        assert_eq!(<Two as ServiceList>::declare_voice(0, 1, [0; 8], 0), Some(OK));
        assert_eq!(*log(), vec![Call::DeclareVoice { svc: 1, rec: 0, ty: 1 }]);
    }
}

#[test]
fn a_tick_phase_reaches_every_service_in_list_order() {
    let _g = LOCK.lock().unwrap();
    unsafe {
        reset();
        <Two as ServiceList>::tick(tick_phase::EARLY);
        <Two as ServiceList>::tick(tick_phase::LATE);
        assert_eq!(
            *log(),
            vec![
                Call::Tick { svc: 1, phase: tick_phase::EARLY },
                Call::Tick { svc: 2, phase: tick_phase::EARLY },
                Call::Tick { svc: 1, phase: tick_phase::LATE },
                Call::Tick { svc: 2, phase: tick_phase::LATE },
            ]
        );
    }
}
