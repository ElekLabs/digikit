//! The lifecycle events of a boot over real packed files: the module and the service it needs each have a begin and an end for the steps of their bring-up, named by their file.
use super::*;
use crate::dlog::{self, Ev};
use crate::package::lifecycle::stem_hash;
use dkrt_lifecycle::{Phase as LPhase, Step};
use dkrt_sdk::logk;

static mut SEEN: Vec<Ev> = Vec::new();

unsafe extern "C" fn keep(e: *const Ev) {
    if (*e).kind == logk::LIFECYCLE {
        (*core::ptr::addr_of_mut!(SEEN)).push(*e);
    }
}

/// (phase, step, package record, name hash, status) of an event.
fn parts(e: &Ev) -> (u32, u32, u32, u32, i32) {
    (e.a[2] & 0xff, e.a[2] >> 8 & 0xff, e.a[2] >> 24, e.a[1], e.a[3] as i32)
}

#[test]
fn a_boot_logs_a_begin_and_an_end_for_each_step_of_a_module_and_its_service() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            (*core::ptr::addr_of_mut!(SEEN)).clear();
            let (mut l, plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            l.capture(0, &packed(dev, LOCAL_PING_USER.leaf, "dkm", LOCAL_PING_USER.nx));
            dlog::dk_debug_log_sink_set(keep as *const () as usize as *mut u8, core::ptr::null_mut());
            let run = boot_run(&mut l, &plan, &[&LOCAL_PING_USER], false);
            dlog::test_reset_sinks();
            assert_eq!(run.verdict(0), Verdict::Accepted);
            let seen: Vec<_> = (*core::ptr::addr_of!(SEEN)).iter().map(parts).collect();
            let service = (MAX_MODULES) as u32;
            for (record, file) in [(service, "services/local-ping.dks\0"), (0, "modules/local-ping-user.dkm\0")] {
                let mine: Vec<_> = seen.iter().filter(|p| p.2 == record).collect();
                let hash = stem_hash(file.as_ptr());
                assert!(mine.iter().all(|p| p.3 == hash), "{}: every event names {file:?}: {mine:?}", dev.name);
                let begins: Vec<_> = mine.iter().filter(|p| p.0 == LPhase::Begin as u32).map(|p| p.1).collect();
                for want in [Step::Place, Step::Start, Step::Install, Step::Activate] {
                    assert!(begins.contains(&(want as u32)), "{}: {file:?} has no begin of {want:?}: {begins:?}", dev.name);
                }
                // each begin is followed by its end, and the step of a successful bring-up answers 0
                for pair in mine.chunks(2) {
                    assert_eq!((pair[0].0, pair[1].0, pair[0].1), (LPhase::Begin as u32, LPhase::End as u32, pair[1].1), "{}: {file:?}: {mine:?}", dev.name);
                    assert_eq!(pair[1].4, 0, "{}: {file:?}: {pair:?}", dev.name);
                }
            }
        }
        clean_state();
    }
}
