//! The commands and imports that queue a live job, and what it counts.
use super::*;

/// The wire mapping of the live commands (`admin_cmds`): the queue's refusal is the reply status, and `RELOAD_STATE` carries phase, result, step, torn and op of the job as bytes.
/// The end of a job counts in `DkrtBootInfo.reloads` when the module set changed: a finished job (`ok`) or one that tore a module down (`changed`), not a refusal.
#[test]
fn a_job_that_changed_the_module_set_counts_one_reload_and_a_refusal_counts_none() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let n = || crate::boot::info().reloads;
        let start = n();
        RealOps.finish(false, false);
        assert_eq!(n(), start);
        RealOps.finish(true, false);
        assert_eq!(n(), start.wrapping_add(1));
        RealOps.finish(false, true);
        assert_eq!(n(), start.wrapping_add(2));
    }
}

#[test]
fn the_live_commands_answer_with_the_jobs_refusal_and_state_in_the_reload_state_body() {
    use crate::package::live_cmds::{cmd_load, cmd_reload, cmd_reload_state, cmd_unload};
    use crate::transport::{DKRT_UPLOAD_LAST, RBODY};
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let status = || unsafe { core::ptr::read_volatile(core::ptr::addr_of!(DKRT_UPLOAD_LAST)) };
    let body = |i: usize| unsafe { *(core::ptr::addr_of!(RBODY) as *const u8).add(1 + i) };
    unsafe {
        *job() = Job::new();
        let path = b"mods/x.dkm\0";
        // no loaded module came from the path: reload and unload say E_NOT_FOUND, an empty body is E_MSG, and no job is queued
        cmd_reload(1, path.as_ptr(), path.len());
        assert_eq!(status(), E_NOT_FOUND);
        cmd_unload(1, path.as_ptr(), path.len());
        assert_eq!(status(), E_NOT_FOUND);
        cmd_load(1, b"\0".as_ptr(), 1);
        assert_eq!(status(), dkrt_sdk::proto::E_MSG);
        cmd_reload_state(1);
        assert_eq!((status(), body(0), body(6), body(7)), (OK, 0, 0, 0), "no job yet");
        // a finished unload that was refused by playback: result, step, torn and op in the body
        *job() = Job::in_state(5, 2, E_RELOAD_PLAYING, true, LIVE_UNLOAD as u8);
        cmd_reload_state(2);
        assert_eq!(status(), OK);
        assert_eq!(body(0), 2, "phase");
        assert_eq!(i32::from_be_bytes([body(1), body(2), body(3), body(4)]), E_RELOAD_PLAYING, "result");
        assert_eq!((body(5), body(6), body(7)), (5, 1, LIVE_UNLOAD as u8), "step, torn, op");
        *job() = Job::new();
    }
}

#[test]
fn a_service_queues_a_live_job_and_reads_its_state_through_the_core_imports() {
    use crate::package::svc_imports::shims::{dkc_live, dkc_live_state};
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // `dkrt_live` runs `init_once`, which sets up the arena at a 32-bit address that a 64-bit host cannot write: the runtime counts as initialised
        crate::platform::st().magic = crate::state::STATE_MAGIC;
        *job() = Job::new();
        // the refusals are those of `dkrt_live`: no loaded module came from the path, an empty path, an unknown op; nothing is queued
        assert_eq!(dkc_live(LIVE_RELOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(dkc_live(LIVE_UNLOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(dkc_live(LIVE_RELOAD, b"\0".as_ptr()), dkrt_sdk::proto::E_MSG);
        assert_eq!(dkc_live(99, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
        assert_eq!(job().phase, 0);
        // a queued job (the state a successful queue leaves) is read the way `dkrt_live_state` reports it
        *job() = Job::in_state(step::CHECK, 1, OK, false, LIVE_LOAD as u8);
        assert_eq!(dkc_live(LIVE_LOAD, b"mods/y.dkm\0".as_ptr()), E_BUSY, "one job at a time");
        let mut out = core::mem::MaybeUninit::<DkrtLive>::uninit();
        dkc_live_state(out.as_mut_ptr());
        let out = out.assume_init();
        assert_eq!((out.phase, out.step, out.op, out.unloaded), (1, step::CHECK as u32, LIVE_LOAD, 0));
        *job() = Job::in_state(5, 2, E_RELOAD_PLAYING, true, LIVE_UNLOAD as u8);
        let mut out = core::mem::MaybeUninit::<DkrtLive>::uninit();
        dkc_live_state(out.as_mut_ptr());
        let out = out.assume_init();
        assert_eq!((out.phase, out.result, out.step, out.unloaded, out.op), (2, E_RELOAD_PLAYING, 5, 1, LIVE_UNLOAD));
        *job() = Job::new();
    }
}

#[test]
fn a_service_cannot_load_or_unload_the_file_of_a_loaded_service_and_a_reload_of_it_is_queued() {
    use crate::package::svc_imports::shims::dkc_live;
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        crate::platform::st().magic = crate::state::STATE_MAGIC;
        *job() = Job::new();
        // a loaded service: the first provider record, with its path
        let rec = dkrt_abi::MAX_MODULES;
        let path = b"services/svc.dks\0";
        crate::package::loader::records::module(rec).used = 1;
        crate::package::loader::records::path(rec)[..path.len()].copy_from_slice(path);
        let answers = [LIVE_LOAD, LIVE_UNLOAD].map(|op| dkc_live(op, path.as_ptr()));
        assert_eq!(answers, [E_FORMAT; 2]);
        assert_eq!(job().phase, 0, "nothing is queued");
        assert_eq!(dkc_live(LIVE_RELOAD, path.as_ptr()), OK);
        assert_eq!((job().phase, job().op), (1, LIVE_RELOAD as u8), "the reload of a service is queued");
        *job() = Job::new();
        crate::package::loader::records::module(rec).used = 0;
        crate::package::loader::records::path(rec)[..path.len()].fill(0);
    }
}

#[test]
fn queueing_resolves_the_path_refuses_a_service_file_a_bad_path_and_a_second_job() {
    let mut s = Script::new();
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"a\0".as_ptr()), OK);
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"b\0".as_ptr()), E_BUSY);
        let mut k = Job::new();
        assert_eq!(k.queue(&mut s, LIVE_RELOAD, b"\0".as_ptr()), dkrt_sdk::proto::E_MSG);
        let mut v = [b'x'; 70].to_vec();
        v.push(0);
        assert_eq!(k.queue(&mut s, LIVE_RELOAD, v.as_ptr()), dkrt_sdk::proto::E_MSG);
        // no loaded module came from the path: the reply says so at once, and no job is queued
        s.found = None;
        assert_eq!(k.queue(&mut s, LIVE_RELOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(k.queue(&mut s, LIVE_UNLOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(k.phase, 0);
        // a load of a path that is loaded is `E_LOADED`; an operation that does not exist is `E_MSG`
        s.found = Some((2, 5));
        assert_eq!(k.queue(&mut s, LIVE_LOAD, b"mods/x.dkm\0".as_ptr()), E_LOADED);
        assert_eq!(k.queue(&mut s, 0, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
        assert_eq!(k.queue(&mut s, 9, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
        assert_eq!(k.phase, 0);
        // the file of a service is no module: only a reload takes it (`tests/service.rs`)
        s.service = true;
        for op in [LIVE_LOAD, LIVE_UNLOAD] {
            assert_eq!(k.queue(&mut s, op, b"services/fx.dks\0".as_ptr()), E_FORMAT);
        }
        assert_eq!(k.phase, 0);
        s.service = false;
        // a finished job takes the next one
        while j.phase == 1 {
            j.poll(&mut s);
        }
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"c\0".as_ptr()), OK);
    }
}
