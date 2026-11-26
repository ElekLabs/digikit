//! The traced seams over the recording fakes: each call the manager makes has its begin and end around it, in the order the seams were called.

extern crate std;

use crate::fake::{traced_manager, Call, Event, Pkg};
use crate::{PackageId, Phase, Step};
use std::vec::Vec;

const V1: u32 = 0x010000;

fn id(n: u8) -> PackageId {
    PackageId(n)
}

fn world() -> (
    crate::fake::TracedManager,
    crate::fake::Probe,
    crate::fake::RecordingTrace,
) {
    traced_manager(&[
        Pkg::service(1, "ref", V1).resource("slots", 4),
        Pkg::module(2).needs("ref", V1),
    ])
}

/// The step a fake call is, `None` for the calls that are no traced step (`ServicesUp`, `Admit`, `StartDsp`, which is a note).
fn step_of(c: Call) -> Option<(Step, PackageId)> {
    Some(match c {
        Call::Place(p) => (Step::Place, p),
        Call::Link(p) => (Step::Link, p),
        Call::Start(p) => (Step::Start, p),
        Call::Install(p) => (Step::Install, p),
        Call::Activate(p) => (Step::Activate, p),
        Call::Deactivate(p) => (Step::Deactivate, p),
        Call::Attach(p) => (Step::Attach, p),
        Call::CanStop(p) => (Step::CanStop, p),
        Call::Detach(p) => (Step::Detach, p),
        Call::Restore(p) => (Step::Restore, p),
        Call::Stop(p) => (Step::Stop, p),
        Call::ExportState(p) => (Step::ExportState, p),
        Call::ImportState(p) => (Step::ImportState, p),
        Call::Unbind(p) => (Step::Unbind, p),
        Call::Free(p) => (Step::Free, p),
        _ => return None,
    })
}

/// The steps the seams were called with, in order, as the fakes saw them.
fn called(probe: &crate::fake::Probe) -> Vec<(Step, PackageId)> {
    probe.calls().into_iter().filter_map(step_of).collect()
}

/// Every begin is followed at once by the end of the same step (the seams do not nest), and the pairs are the calls in order.
fn assert_pairs_are(events: &[Event], expected: &[(Step, PackageId)]) {
    assert_eq!(events.len(), expected.len() * 2, "{events:?}");
    for (pair, (step, package)) in events.chunks(2).zip(expected) {
        assert_eq!(
            (pair[0].phase, pair[0].step, pair[0].package),
            (Phase::Begin, *step, *package)
        );
        assert_eq!(
            (pair[1].phase, pair[1].step, pair[1].package),
            (Phase::End, *step, *package)
        );
    }
}

#[test]
fn a_load_has_a_begin_and_an_end_for_every_call_in_order() {
    let (mut pm, probe, trace) = world();
    pm.boot(&[id(1)]).unwrap();
    probe.clear();
    trace.clear();
    pm.load(id(2)).unwrap();
    let expected = called(&probe);
    assert!(expected.contains(&(Step::Place, id(2))) && expected.contains(&(Step::Attach, id(2))));
    assert_pairs_are(&trace.events(), &expected);
    assert!(trace
        .events()
        .iter()
        .all(|e| e.phase == Phase::Begin || e.status == 0));
}

#[test]
fn an_unload_has_a_begin_and_an_end_for_every_call_in_order() {
    let (mut pm, probe, trace) = world();
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    trace.clear();
    pm.unload(id(2)).unwrap();
    let expected = called(&probe);
    assert!(expected.contains(&(Step::Detach, id(2))) && expected.contains(&(Step::Free, id(2))));
    assert_pairs_are(&trace.events(), &expected);
}

#[test]
fn a_reload_has_a_begin_and_an_end_for_every_call_in_order_and_exports_the_state() {
    let (mut pm, probe, trace) = world();
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    trace.clear();
    pm.reload(id(1)).unwrap();
    let expected = called(&probe);
    assert!(
        expected.contains(&(Step::ExportState, id(1)))
            && expected.contains(&(Step::ImportState, id(1)))
    );
    assert_pairs_are(&trace.events(), &expected);
}

#[test]
fn a_failing_step_ends_with_its_status() {
    let (mut pm, probe, trace) = world();
    pm.boot(&[id(1)]).unwrap();
    probe.fail(Call::Start(id(2)), -5);
    trace.clear();
    assert!(pm.load(id(2)).is_err());
    let ev = trace.events();
    let end = ev
        .iter()
        .find(|e| e.step == Step::Start && e.phase == Phase::End)
        .expect("start ended");
    assert_eq!((end.package, end.status), (id(2), -5));
    assert!(
        !ev.iter().any(|e| e.step == Step::Attach),
        "nothing after the failed start but the unwind: {ev:?}"
    );
}

#[test]
fn nothing_is_emitted_while_the_trace_is_off() {
    let (mut pm, probe, trace) = world();
    pm.boot(&[id(1)]).unwrap();
    trace.clear();
    trace.switch_off();
    probe.clear();
    pm.load(id(2)).unwrap();
    assert!(!called(&probe).is_empty(), "the load still ran");
    assert!(trace.events().is_empty());
}

#[test]
fn the_start_of_a_dsp_part_is_a_note() {
    let (mut pm, _probe, trace) = traced_manager(&[Pkg::service(1, "ref", V1).with_dsp()]);
    pm.boot(&[id(1)]).unwrap();
    let notes: Vec<_> = trace
        .events()
        .into_iter()
        .filter(|e| e.phase == Phase::Note)
        .collect();
    assert_eq!(
        notes,
        [Event {
            phase: Phase::Note,
            step: Step::DspLoad,
            package: id(1),
            status: 0
        }]
    );
}
