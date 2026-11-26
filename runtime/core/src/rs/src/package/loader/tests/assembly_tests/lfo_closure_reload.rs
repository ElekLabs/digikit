//! The closure reload of `lfo.dks` with a record-owning consumer: the real packed lfo service and the real packed `lfo-shape` fixture (which adds a record the service owns) are
//! booted, then the live job `RELOAD` of `services/lfo.dks` (the same file) runs over `HostOps`. A device hang of this reload (the shape module placed, its link begun and never
//! ended) is reproduced or ruled out here: every lifecycle step of the shape module must begin and end in the boot and again in the reload, the job must end `OK` within a bound of
//! polls (`reload_job` fails the test, it does not hang it), the shape's key number must be unchanged and the service must be up again.
//!
//! The device refuses this reload (`E_BOOT_ONLY`: the lfo hooks sit at interrupt sites, `provider_reload::interrupt_entered`); the test switches that one rule off with the loader's
//! test field `irq_guard_off` and leaves the rule's own source alone.
use super::*;
use crate::dlog::{self, Ev};
use crate::package::lifecycle::{stem_hash, Rt};
use crate::package::reload::{step, Job};
use dkrt_lifecycle::{Phase as LPhase, Step};
use dkrt_sdk::logk;

static mut SEEN: Vec<Ev> = Vec::new();

unsafe extern "C" fn keep(e: *const Ev) {
    if (*e).kind == logk::LIFECYCLE {
        (*core::ptr::addr_of_mut!(SEEN)).push(*e);
    }
}

/// (operation, phase, step, package record, name hash, status) of a lifecycle event.
fn parts(e: &Ev) -> (u32, u32, u32, u32, u32, i32) {
    (e.a[0], e.a[2] & 0xff, e.a[2] >> 8 & 0xff, e.a[2] >> 24, e.a[1], e.a[3] as i32)
}

/// Every begin of the events of `file` in operation `op` is followed by its end (same step) with status 0; the steps `want` all occur.
fn steps_end(what: &str, seen: &[(u32, u32, u32, u32, u32, i32)], op: u32, file: &str, want: &[Step]) {
    let hash = unsafe { stem_hash(file.as_ptr()) };
    let mine: Vec<_> = seen.iter().filter(|p| p.0 == op && p.4 == hash && p.3 != 0xff && p.2 < Step::JobCheck as u32).collect();
    for w in want {
        assert!(mine.iter().any(|p| p.1 == LPhase::Begin as u32 && p.2 == *w as u32), "{what}: op {op}: {file} has no begin of {w:?}: {mine:?}");
        assert!(mine.iter().any(|p| p.1 == LPhase::End as u32 && p.2 == *w as u32 && p.5 == 0), "{what}: op {op}: {file} has no good end of {w:?}: {mine:?}");
    }
    let (mut open, mut ends) = (Vec::new(), 0);
    for p in &mine {
        if p.1 == LPhase::Begin as u32 {
            open.push(p.2);
        } else if p.1 == LPhase::End as u32 {
            ends += 1;
            let at = open.iter().rposition(|s| *s == p.2);
            assert!(at.is_some(), "{what}: op {op}: {file}: end of step {} without a begin: {mine:?}", p.2);
            open.remove(at.unwrap());
        }
    }
    assert!(open.is_empty() && ends > 0, "{what}: op {op}: {file}: steps begun and never ended {open:?}: {mine:?}");
}

#[test]
fn the_closure_reload_of_the_lfo_service_brings_the_shape_module_up_again_through_every_step() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            (*core::ptr::addr_of_mut!(SEEN)).clear();
            let (mut l, plan) = planned(dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(dev, &l, &plan);
            let rec = boot_record(&[LFO_SHAPE.leaf]);
            let mut rt = Box::new(Rt::new(&mut l as *mut _));
            dlog::dk_debug_log_sink_set(keep as *const () as usize as *mut u8, core::ptr::null_mut());
            rt.boot_with(&rec, &capacity(&l.p));
            let lfo = (0..plan.np).find(|j| l.found.path(*j) == b"services/lfo.dks").expect("lfo.dks is found") + MAX_MODULES;
            assert!(rt.manager().unwrap().live().contains(PackageId(0)) && rt.manager().unwrap().live().contains(PackageId(lfo as u8)), "{}: the boot brought both up", dev.name);
            let mut keys = [0u32; 8];
            let n = l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut keys);
            assert_eq!(n, 1, "{}: the shape module has one wave key", dev.name);
            let wave = keys[0];

            l.irq_guard_off = true;
            let path = b"services/lfo.dks\0";
            let mut job = Job::new();
            let mut ops = super::super::host_ops::HostOps { l: &mut l, rt: &mut rt };
            let before = (*core::ptr::addr_of!(SEEN)).len();
            assert_eq!(job.queue(&mut ops, LIVE_RELOAD, path.as_ptr()), OK, "{}: the job takes the reload", dev.name);
            let mut polls = 0;
            while job.phase == 1 && polls < 8000 {
                job.poll(&mut ops);
                ops.l.p.tick(25);
                polls += 1;
            }
            let (phase, at, result) = (job.phase, job.step, job.result);
            dlog::test_reset_sinks();
            let seen: Vec<_> = (*core::ptr::addr_of!(SEEN)).iter().map(parts).collect();
            let tail = &seen[before.min(seen.len())..];
            assert!(phase == 2 && at == step::DONE, "{}: the job did not end in {polls} polls: phase {phase} step {at}; events of the reload: {tail:?}", dev.name);
            assert_eq!(result, OK, "{}: the reload ended {result}", dev.name);

            let boot_op = seen[0].0;
            let reload_op = tail.first().expect("the reload logged events").0;
            assert_ne!(boot_op, reload_op, "{}", dev.name);
            let steps = [Step::Place, Step::Link, Step::Start, Step::Install, Step::Activate];
            for (op, what) in [(boot_op, "boot"), (reload_op, "reload")] {
                steps_end(&std::format!("{} {what}", dev.name), &seen, op, "services/lfo.dks\0", &steps);
            }
            // the end of a place carries the base of the placed image
            let shape = unsafe { stem_hash(b"modules/lfo-shape.dkm\0".as_ptr()) };
            for e in (*core::ptr::addr_of!(SEEN)).iter().filter(|e| parts(e).4 == shape && parts(e).1 == LPhase::End as u32 && parts(e).2 == Step::Place as u32) {
                assert_ne!(e.a[4], 0, "{}: the place end names the base of the image", dev.name);
            }
            for (op, what) in [(boot_op, "boot"), (reload_op, "reload")] {
                steps_end(&std::format!("{} {what}", dev.name), &seen, op, "modules/lfo-shape.dkm\0", &[Step::Place, Step::Link, Step::Start, Step::Attach]);
            }

            let m = rt.manager().unwrap();
            assert!(m.live().contains(PackageId(0)) && m.live().contains(PackageId(lfo as u8)), "{}: both are up again", dev.name);
            let mut again = [0u32; 8];
            assert_eq!(l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut again), 1, "{}", dev.name);
            assert_eq!(again[0], wave, "{}: the wave key of the shape is unchanged", dev.name);
        }
        clean_state();
    }
}
