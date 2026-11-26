//! The package manager over the runtime's real seams and the real packed `local-ping` fixtures (docs/plans/runtime-contract-and-lifecycle.md, RC2b-1): the service and
//! two consumers as `patchkit pack` leaves them, on `FakePlatform`, through the same checks, role steps and commits as the boot's load loop. What the host cannot show stays
//! out (no ColdFire code runs: `init` of the service replays its declarations, `init` of a module is module code). The cases: bring-up from the selection, the unload of
//! the first consumer (the service stays for the second), the unload of the last (the orphaned service follows), and the memory afterwards; and the hooks of `svc-test`
//! (a ColdFire entry): the installer holds them after the bring-up and the firmware bytes are stock after the unload.
//!
//! RC2b-2 adds the runtime's own driver over the manager (`Rt`): the boot with a package that does not come up (the rest loads and the boot record names the cause), a DSP part
//! that fails (the closure goes down), and the live job's steps for a module (`unload`, then `reload_load` and `live_load`: the services stay up).
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware image is skipped.
use super::assembly_tests::*;
use super::*;
use crate::package::lifecycle::{Rt, RtManifest, Session};
use dkrt_lifecycle::{PackageId, PackageManager};

/// The attach and detach counters of the provider fixture, asked through the query part of its row in the runtime's table.
fn ping_counters() -> (u32, u32) {
    let hash = dkneeds::keys::name_hash("local-ping");
    let h = crate::package::provider_rows::rows().iter().find(|r| r.name() == hash).expect("the row of local-ping is in the table");
    let mut reply = [0u8; dkrt_fixture_local_ping::COUNTERS_LEN];
    assert_eq!(unsafe { h.query(dkrt_fixture_local_ping::OP_COUNTERS, &[], &mut reply) }, reply.len() as i32);
    let word = |i: usize| u32::from_be_bytes([reply[i * 4], reply[i * 4 + 1], reply[i * 4 + 2], reply[i * 4 + 3]]);
    (word(0), word(1))
}

fn id(n: usize) -> PackageId {
    PackageId(n as u8)
}

#[test]
fn the_manager_brings_up_the_packed_local_ping_fixtures_and_takes_them_down_with_the_orphan() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &LOCAL_PING_USER_B], &[LOCAL_PING]);
            assert_eq!(l.found.n, 1, "{}: the search finds the service the consumers need", dev.name);
            let stock = l.p.code.borrow().clone();
            let paths: Vec<Vec<u8>> = [&LOCAL_PING_USER, &LOCAL_PING_USER_B].iter().map(|m| std::format!("modules/{}.dkm\0", m.leaf).into_bytes()).collect();
            let mut at = [None; MAX_MODULES];
            for (k, p) in paths.iter().enumerate() {
                at[k] = Some(p.as_ptr());
            }
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 2);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let (user, user_b, service) = (id(0), id(1), id(MAX_MODULES));

            // bring-up: the service first, then the consumers, each pinned by the selection
            let report = pm.boot(&[user, user_b]).unwrap();
            assert!(report.refused().is_empty(), "{}: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!(report.brought_up().as_slice(), [service, user, user_b], "{}", dev.name);
            let hash = dkneeds::keys::name_hash("local_ping_value");
            assert!(l.exports.resolve(hash).is_none(), "the export is bound to its consumers, not global");
            assert!(l.exports.owned(hash).is_some(), "{}: the service published its export", dev.name);
            assert!(crate::package::provider_rows::has("local-ping"), "{}: its row is in the table", dev.name);
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [1, 1, 1]);
            assert_eq!(st().loads, 2, "two modules are loaded");
            // the provider heard both consumers come up (`Provider::attach`, also without records) and nothing has gone; its query answers the counters
            assert_eq!(ping_counters(), (2, 0), "{}", dev.name);
            // the consumer's import word is the export's address
            let export = l.exports.owned(hash).unwrap().0;
            let found_export = l.m.live.iter().any(|(_, mem)| {
                let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, mem.len() * 16);
                words.windows(4).any(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) == export)
            });
            assert!(found_export, "{}: a consumer's image holds the address of the service's export", dev.name);

            // the first consumer goes, the service stays for the second
            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user], "{}", dev.name);
            assert_eq!(ping_counters(), (2, 1), "{}: the first consumer detached from the provider", dev.name);
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [0, 1, 1]);

            // the last consumer goes and the orphaned service follows
            let report = pm.unload(user_b).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user_b, service], "{}", dev.name);
            assert!(!s.retained(user) && !s.retained(user_b) && !s.retained(service));
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [0, 0, 0]);
            assert_eq!(l.exports.count(), 0, "the export is withdrawn");
            assert!(!crate::package::provider_rows::has("local-ping"), "its row is gone");
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
            assert_eq!(l.installer.count(), 0);
            assert_eq!(*l.p.code.borrow(), stock, "{}: the firmware is as it was", dev.name);
        }
        clean_state();
    }
}

static mut GONE: Vec<u32> = Vec::new();
unsafe extern "C" fn observer(owner: u32) {
    (*core::ptr::addr_of_mut!(GONE)).push(owner);
}

/// A packed service file is an owner for core/ui as a module is: its context names its record (`dkrt_ctx_owner`) and the unload observers are told when it goes down, after
/// the consumers. One that fits (the service with its consumer) and the typical refusal (the context of a record that is not loaded).
#[test]
fn a_packed_service_is_an_owner_and_its_unload_reaches_the_observers() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let (user, service) = (id(0), id(MAX_MODULES));
            (*core::ptr::addr_of_mut!(GONE)).clear();
            assert_eq!(crate::api::dkrt_unload_observer_add(observer as *const () as usize), dkrt_sdk::OK);
            let report = pm.boot(&[user]).unwrap();
            assert_eq!(report.brought_up().as_slice(), [service, user], "{}", dev.name);

            let ctx_of = |rec: usize| core::ptr::addr_of!(crate::package::entry::DKRT_CTXS[rec]) as usize;
            assert_eq!(crate::api::dkrt_ctx_owner(ctx_of(MAX_MODULES)), MAX_MODULES as i32, "{}: the service's context names its record", dev.name);
            assert_eq!(crate::api::dkrt_ctx_owner(ctx_of(MAX_MODULES) + 1), dkrt_sdk::E_CONTRACT, "{}: a pointer into the context is none", dev.name);

            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user, service], "{}", dev.name);
            assert_eq!(*core::ptr::addr_of!(GONE), [0, MAX_MODULES as u32], "{}: the observer heard the module, then the service", dev.name);
            crate::api::dkrt_unload_observer_remove(observer as *const () as usize);
            assert_eq!(crate::api::dkrt_ctx_owner(ctx_of(MAX_MODULES)), dkrt_sdk::E_CONTRACT, "{}: a record that is gone has no owner", dev.name);
        }
        clean_state();
    }
}

#[test]
fn the_manager_installs_the_hooks_of_a_service_and_the_firmware_is_stock_after_the_unload() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&SVC_USER], &[SVC_TEST]);
            assert_eq!(l.found.n, 2, "{}: the search finds the service the consumer needs and the port file", dev.name);
            let stock = l.p.code.borrow().clone();
            let path = std::format!("modules/{}.dkm\0", SVC_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            // the services of the search in the order of `found`: svc-test, then the port file that its hooks name
            let (user, service, port) = (id(0), id(MAX_MODULES), id(MAX_MODULES + 1));
            crate::package::boot_idx::set(u32::MAX);

            let report = pm.boot(&[user]).unwrap();
            assert!(report.refused().is_empty(), "{}: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!(report.brought_up().as_slice(), [port, service, user], "{}", dev.name);
            assert_eq!(crate::package::boot_idx::get(), u32::MAX, "{}: the bring-up leaves the boot index as it was", dev.name);
            // the hooks of the service are in: the installer holds its sites and the firmware is not stock any more
            assert!(l.installer.count() > 0, "{}: the installer recorded the hooks of the test service", dev.name);
            assert_ne!(*l.p.code.borrow(), stock, "{}: the hook sites are patched", dev.name);

            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user, service, port], "{}", dev.name);
            assert_eq!(l.installer.count(), 0, "{}", dev.name);
            assert_eq!(*l.p.code.borrow(), stock, "{}: the firmware bytes are stock after the unload", dev.name);
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
            assert_eq!(crate::package::boot_idx::get(), u32::MAX, "{}: the unload leaves the boot index as it was", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_provider_row_with_a_part_newer_than_the_core_knows_is_refused_and_leaves_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            l.m.newer_parts = true;
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let report = pm.boot(&[id(0)]).unwrap();
            assert!(report.brought_up().is_empty(), "{}", dev.name);
            assert!(
                report.refused().iter().any(|r| matches!(r.reason, dkrt_lifecycle::LifecycleError::Failed { status, .. } if status == dkrt_sdk::E_SVC_ABI)),
                "{}: the provider is refused with the status of a newer part: {:?}",
                dev.name,
                report.refused().iter().map(|r| r.reason).collect::<Vec<_>>()
            );
            assert!(!crate::package::provider_rows::has("local-ping"), "{}: no row stays", dev.name);
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
            assert_provider_stopped(&l, dev.name);
        }
        clean_state();
    }
}

/// The file's `init` ran (it made the owner object) and its commit refused the row: the loader ran the file's `fini`, so `Owner::stop` ran and the cell is empty.
fn assert_provider_stopped(l: &Boot, dev: &str) {
    assert!(l.m.hooks.iter().any(|(fini, _)| *fini), "{dev}: the loader ran the fini of a provider whose commit was refused after its init");
    assert!(dkrt_fixture_local_ping::DK_SVC_OWNER.instance().is_null(), "{dev}: the owner cell is empty");
}

#[test]
fn a_provider_row_that_names_another_service_is_refused_and_its_owner_is_stopped() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            l.m.wrong_name = true;
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let report = pm.boot(&[id(0)]).unwrap();
            assert!(report.brought_up().is_empty(), "{}", dev.name);
            assert!(!report.refused().is_empty(), "{}", dev.name);
            assert!(!crate::package::provider_rows::has("local-ping"), "{}: no row stays", dev.name);
            assert_provider_stopped(&l, dev.name);
        }
        clean_state();
    }
}

#[test]
fn the_boot_continues_with_the_rest_when_a_service_does_not_come_up_and_reports_each_module() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // module 0 needs `local-ping`, whose file is damaged after the plan read its table; module 1 needs `svc-test`, which is fine
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &SVC_USER], &[LOCAL_PING, SVC_TEST]);
            let file = l.m.files.iter_mut().find(|(n, _)| n.starts_with(b"services/local-ping")).unwrap();
            file.1[0] ^= 0xff;
            let rec = boot_record(&[LOCAL_PING_USER.leaf, SVC_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let info = crate::boot::info();
            assert_eq!(info.results[0], dkrt_sdk::E_MISSING, "{}: the consumer of the refused service is refused with the planner's code", dev.name);
            assert_eq!(info.results[1], dkrt_sdk::OK, "{}: the other module loads", dev.name);
            assert_eq!(info.states[0], dkrt_sdk::STATE_REFUSED, "{}", dev.name);
            assert_eq!(info.states[1], dkrt_sdk::STATE_ACTIVE, "{}", dev.name);
            assert_eq!(info.loaded, 1);
            assert_eq!([records::module(0).used, records::module(1).used], [0, 1], "{}: the record follows the selection index", dev.name);
            let name = core::slice::from_raw_parts(crate::boot::boot_name(1), 17);
            assert_eq!(name, &b"svc-test-user.dkm"[..], "{}", dev.name);
            // the manager holds the survivors and nothing of the refused one
            let m = rt.manager().unwrap();
            assert!(m.live().contains(id(1)) && !m.live().contains(id(0)));
            assert!(m.graph().node(id(1)).unwrap().is_pinned(), "the selection pins the modules that came up");
        }
        clean_state();
    }
}

#[test]
fn a_failed_dsp_part_takes_down_the_closure_of_its_package_and_marks_the_modules_that_needed_it() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &LOCAL_PING_USER_B], &[LOCAL_PING]);
            let rec = boot_record(&[LOCAL_PING_USER.leaf, LOCAL_PING_USER_B.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let service = id(MAX_MODULES);
            assert_eq!(crate::boot::info().loaded, 2, "{}", dev.name);
            // the DSP part of the service waits in the DSP loader's queue and is given up (the path of a failed transfer, which takes the same closure down)
            crate::package::dsp::ds().nwait = 1;
            crate::package::dsp::ds().waiting[0].rec = service.0 as u32;
            assert!(rt.cancel_dsp(service.0 as usize, dkrt_sdk::E_DSP_XFER), "{}", dev.name);
            let m = rt.manager().unwrap();
            assert!(m.live().is_empty(), "{}: the consumers went first and the service with them", dev.name);
            assert_eq!([records::module(0).used, records::module(1).used, records::module(MAX_MODULES).used], [0, 0, 0], "{}", dev.name);
            let info = crate::boot::info();
            assert_eq!([info.states[0], info.states[1]], [dkrt_sdk::STATE_FAILED; 2], "{}: both modules are FAILED", dev.name);
            assert_eq!([info.results[0], info.results[1]], [dkrt_sdk::E_MISSING; 2], "{}: with the code of a service that failed", dev.name);
            assert_eq!(info.loaded, 0);
            assert!(!crate::package::dsp::retained(), "{}", dev.name);
            assert!(l.m.live.is_empty(), "{}: every block is back", dev.name);
            assert!(!rt.session().retained(id(0)) && !rt.session().retained(service));
        }
        clean_state();
    }
}

/// A restore that answers busy inside the take-down of a failed DSP part (a task below the UI task is not parked) does not retain the DSP state: the load stays pending, a later pass
/// finishes the take-down, the DSP loader goes idle and the next job is not refused with `E_RELOAD_RETAINED`.
#[test]
fn a_busy_restore_in_the_take_down_of_a_failed_dsp_part_keeps_it_pending_and_a_later_pass_ends_it() {
    use crate::package::dsp;
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the service has hooks: its take-down restores them (a package without entries has nothing to restore)
            let (mut l, _plan) = planned(dev, &image, &[], &[&SVC_USER], &[SVC_TEST]);
            let rec = boot_record(&[SVC_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let (user, service) = (id(0), id(MAX_MODULES));
            assert!(rt.manager().unwrap().live().contains(service), "{}", dev.name);
            assert!(l.installer.count() > 0, "{}: the hooks are in", dev.name);
            // the DSP part of the service is in flight and given up while a restore is busy once
            dsp::ds().phase = dsp::PH_ROWS;
            dsp::ds().pending.rec = service.0 as u32;
            l.busy_restores = 1;
            assert!(rt.cancel_dsp(service.0 as usize, dkrt_sdk::E_DSP_XFER), "{}", dev.name);
            assert!(!dsp::retained(), "{}: a busy restore does not retain the DSP state", dev.name);
            assert_eq!(dsp::ds().phase, dsp::PH_ROWS, "{}: the load stays pending", dev.name);
            assert!(rt.manager().unwrap().restore_pending().is_some(), "{}: the manager holds the take-down", dev.name);
            // the next pass: the tasks are parked, the restore completes and the failure is final
            rt.dsp_step(crate::package::handlers::tests::two());
            assert!(!dsp::retained(), "{}", dev.name);
            assert_eq!(dsp::ds().phase, dsp::PH_IDLE, "{}: the loader is idle again", dev.name);
            let m = rt.manager().unwrap();
            assert!(!m.live().contains(user) && !m.live().contains(service), "{}: the closure is down", dev.name);
            assert_eq!(l.installer.count(), 0, "{}: the hooks are out", dev.name);
            assert_eq!(crate::boot::info().results[0], dkrt_sdk::E_MISSING, "{}", dev.name);
        }
        clean_state();
    }
}

/// A queued DSP load given up while a restore is busy leaves the manager's take-down pending (the DSP loader still steps). A second DSP part that fails in that window gets
/// `RestorePending` from the manager: that is a wait like a busy restore, not a permanent failure that retains the DSP state.
#[test]
fn a_dsp_part_that_fails_while_a_queued_take_down_is_pending_waits_and_does_not_retain_the_dsp_state() {
    use crate::package::dsp;
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&SVC_USER], &[SVC_TEST]);
            let rec = boot_record(&[SVC_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let (user, service) = (id(0), id(MAX_MODULES));
            // the queued DSP load of the service is given up while a restore is busy: nothing is in flight, the take-down stays pending
            dsp::ds().nwait = 1;
            dsp::ds().waiting[0].rec = service.0 as u32;
            l.busy_restores = 1;
            assert!(rt.cancel_dsp(service.0 as usize, dkrt_sdk::E_DSP_XFER), "{}", dev.name);
            assert!(rt.manager().unwrap().restore_pending().is_some(), "{}", dev.name);
            // another DSP part fails in that window
            dsp::ds().phase = dsp::PH_ROWS;
            dsp::ds().pending.rec = user.0 as u32;
            assert!(rt.cancel_dsp(user.0 as usize, dkrt_sdk::E_DSP_XFER), "{}", dev.name);
            assert!(!dsp::retained(), "{}: the refusal while a take-down is pending is a wait", dev.name);
            assert_eq!(dsp::ds().phase, dsp::PH_ROWS, "{}: the load stays pending", dev.name);
            // a later pass finishes the take-down and the failure
            rt.dsp_step(crate::package::handlers::tests::two());
            assert!(!dsp::retained(), "{}", dev.name);
            assert_eq!(dsp::ds().phase, dsp::PH_IDLE, "{}", dev.name);
            let m = rt.manager().unwrap();
            assert!(!m.live().contains(user) && !m.live().contains(service), "{}: the closure is down", dev.name);
        }
        clean_state();
    }
}

#[test]
fn the_live_jobs_steps_unload_a_module_and_load_it_again_and_another_with_the_service_staying_up() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &LOCAL_PING_USER_B], &[LOCAL_PING]);
            // only the first module is selected: the second is a file on the card that a live load adds
            let mut rec = boot_record(&[LOCAL_PING_USER.leaf]);
            rec.count = 1;
            crate::boot::info().count = 1;
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let service = id(MAX_MODULES);
            assert!(rt.manager().unwrap().live().contains(service) && rt.manager().unwrap().live().contains(id(0)), "{}", dev.name);
            assert_eq!(rt.refusal(0), None, "{}: nothing refuses the unload", dev.name);

            // the job's UNLOAD: the module goes, the service its closure does not include stays
            assert_eq!(rt.unload(0), dkrt_sdk::OK, "{}", dev.name);
            let m = rt.manager().unwrap();
            assert!(!m.live().contains(id(0)) && m.live().contains(service), "{}: the service stays with no consumer", dev.name);
            assert_eq!(records::module(0).used, 0);
            assert_eq!(rt.unload(0), dkrt_sdk::OK, "{}: a second ask finds nothing left", dev.name);

            // its LOAD (a reload): the new file's table is read and the module comes back into its own record, the service is not placed again
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            let placed_service = l.provs[0].block.is_some();
            assert_eq!(rt.reload_load_with(0, path.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}", dev.name);
            assert_eq!(records::module(0).used, 1, "{}", dev.name);
            assert!(placed_service && rt.manager().unwrap().live().contains(service), "{}: the service kept its block", dev.name);
            assert!(rt.manager().unwrap().live().contains(id(0)));

            // a live load of the second module takes its own index (a record follows it)
            let path_b = std::format!("modules/{}.dkm\0", LOCAL_PING_USER_B.leaf).into_bytes();
            assert_eq!(rt.live_load_with(1, path_b.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}", dev.name);
            assert_eq!([records::module(0).used, records::module(1).used], [1, 1], "{}", dev.name);
            let m = rt.manager().unwrap();
            assert!(m.live().contains(id(1)) && m.live().contains(service), "{}", dev.name);

            // both go, and the last of them leaves the service up (the live job unloads no service); a file that is not on the card is refused and leaves nothing
            assert_eq!(rt.unload(0), dkrt_sdk::OK);
            assert_eq!(rt.unload(1), dkrt_sdk::OK);
            assert!(rt.manager().unwrap().live().contains(service));
            assert_eq!(rt.live_load_with(2, b"modules/not-there.dkm\0".as_ptr(), &live_cap(&l)), dkrt_sdk::E_NOT_FOUND, "{}", dev.name);
            assert_eq!(records::module(2).used, 0);
        }
        clean_state();
    }
}


/// The packed `local-ping` service with the constant of its one export changed (`moveq #42,d0` is `moveq #43,d0`): the second version of the file, sealed again.
fn local_ping_v2(dev: Dev) -> Vec<u8> {
    let mut f = packed(dev, "local-ping", "dks", "test/local-ping");
    let at = f.windows(4).position(|w| w == [0x70, 0x2a, 0x4e, 0x75]).expect("local_ping_value is moveq #42,d0; rts");
    f[at + 1] = 0x2b;
    seal(&mut f);
    f
}

/// The bytes at device address `addr` in the arena of the card (a block of a live load), `n` of them.
fn arena_bytes(l: &Boot, addr: u32, n: usize) -> Vec<u8> {
    let (base, mem) = l.m.live.iter().find(|(b, mem)| addr >= *b && ((addr - *b) as usize) < mem.len() * 16).expect("the address is in a live block");
    let bytes = unsafe { core::slice::from_raw_parts(mem.as_ptr() as *const u8, mem.len() * 16) };
    bytes[(addr - base) as usize..(addr - base) as usize + n].to_vec()
}

#[test]
fn a_service_reload_runs_the_new_file_and_the_consumer_binds_to_its_export() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            let rec = boot_record(&[LOCAL_PING_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let (user, service) = (id(0), id(MAX_MODULES));
            let hash = dkneeds::keys::name_hash("local_ping_value");
            assert!(rt.manager().unwrap().live().contains(user) && rt.manager().unwrap().live().contains(service), "{}", dev.name);
            let v1 = l.exports.owned(hash).expect("the service published its export").0;
            assert_eq!(arena_bytes(&l, v1, 2), [0x70, 0x2a], "{}: the first version returns 42", dev.name);
            assert_eq!(ping_counters(), (1, 0), "{}", dev.name);
            let table = l.found.table(0).to_vec();

            // the new file is checked before anything goes down, and the check changes nothing
            let path = std::format!("services/{}.dks\0", LOCAL_PING.leaf).into_bytes();
            l.m.files.iter_mut().find(|(n, _)| n == &path[..path.len() - 1]).unwrap().1 = local_ping_v2(dev);
            assert_eq!(rt.refusal(MAX_MODULES), None, "{}: nothing refuses the reload of the service", dev.name);
            assert_eq!(l.provider_check(MAX_MODULES, path.as_ptr()), dkrt_sdk::OK, "{}", dev.name);
            assert_eq!(l.exports.owned(hash).unwrap().0, v1, "{}: the check touched nothing", dev.name);
            assert!(!rt.closure_has_dsp(MAX_MODULES), "{}", dev.name);

            // the closure: the consumer, then the service; the job's call takes both down and brings both back from the new file
            let mut members = [0usize; records::REC_MAX];
            assert_eq!(rt.closure(MAX_MODULES, &mut members), 2);
            assert_eq!(members[..2], [0, MAX_MODULES], "{}: consumers first", dev.name);
            assert_eq!(rt.reload_closure_with(MAX_MODULES, path.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}", dev.name);
            let m = rt.manager().unwrap();
            assert!(m.live().contains(user) && m.live().contains(service), "{}: both are back", dev.name);
            assert_eq!([records::module(0).used, records::module(MAX_MODULES).used], [1, 1], "{}", dev.name);
            // the consumer is bound to the export of the new copy, which returns 43
            let v2 = l.exports.owned(hash).expect("the new service published its export").0;
            assert_eq!(arena_bytes(&l, v2, 2), [0x70, 0x2b], "{}: the second version returns 43", dev.name);
            let bound = l.m.live.iter().any(|(_, mem)| {
                let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, mem.len() * 16);
                words.windows(4).any(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) == v2)
            });
            assert!(bound, "{}: an image holds the address of the new export", dev.name);
            // the owner object was made again by `start_in` and the consumer attached to it
            assert_eq!(ping_counters(), (1, 0), "{}", dev.name);
            assert_eq!(l.found.table(0), &table[..], "{}: the file's table is the same one", dev.name);
            assert!(!rt.reload_torn() && l.exports.resolve(hash).is_none(), "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_service_reload_that_is_refused_before_the_take_down_leaves_both_up_and_a_member_with_a_dsp_part_is_seen() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            let rec = boot_record(&[LOCAL_PING_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let (user, service) = (id(0), id(MAX_MODULES));
            let table = l.found.table(0).to_vec();
            // the new file is not on the card: refused with nothing changed
            assert_eq!(rt.reload_closure_with(MAX_MODULES, b"services/not-there.dks\0".as_ptr(), &live_cap(&l)), dkrt_sdk::E_NOT_FOUND, "{}", dev.name);
            assert_eq!(rt.reload_closure_with(0, b"modules/x.dkm\0".as_ptr(), &live_cap(&l)), dkrt_sdk::E_NOT_FOUND, "{}: a module is no service", dev.name);
            let m = rt.manager().unwrap();
            assert!(m.live().contains(user) && m.live().contains(service), "{}", dev.name);
            assert!(!rt.reload_torn(), "{}", dev.name);
            assert_eq!(l.found.table(0), &table[..], "{}", dev.name);
            // a member with a DSP part: the job refuses the closure (the DSP area range of the record is set)
            let area = crate::package::dsp::ds().mod_len[0];
            crate::package::dsp::ds().mod_len[0] = 64;
            assert!(rt.closure_has_dsp(MAX_MODULES), "{}", dev.name);
            crate::package::dsp::ds().mod_len[0] = area;
            assert!(!rt.closure_has_dsp(MAX_MODULES), "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_busy_module_keeps_its_tick_and_stays_loaded_when_the_live_job_asks_the_manager_to_unload_it() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            let rec = boot_record(&[LOCAL_PING_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let (user, service) = (id(0), id(MAX_MODULES));
            let tick = crate::package::entry::tick_of(0);
            // (a fixture without a tick still shows the record and the manager checks below)
            // the job's QUIESCE stops the tick; the DSP loader holds rows it cannot clear, so the unload is busy
            let _ = l.reload_quiesce(0);
            assert_eq!(crate::package::entry::tick_of(0), 0, "{}: the quiesce stopped the tick", dev.name);
            crate::package::dsp::ds().phase = crate::package::dsp::PH_RETAINED;
            assert_eq!(rt.unload(0), dkrt_sdk::E_BUSY, "{}", dev.name);
            // nothing of the module was taken down: the manager still holds it and its service, the record is whole, and the abort gives the tick back
            let m = rt.manager().unwrap();
            assert!(m.live().contains(user) && m.live().contains(service), "{}: the manager keeps the module", dev.name);
            assert_eq!(records::module(0).used, 1, "{}", dev.name);
            assert!(l.reload_abort(0), "{}", dev.name);
            assert_eq!(crate::package::entry::tick_of(0), tick, "{}: the tick is back", dev.name);
            // once the DSP is free the unload goes through
            crate::package::dsp::ds().phase = crate::package::dsp::PH_IDLE;
            assert_eq!(rt.unload(0), dkrt_sdk::OK, "{}", dev.name);
            assert!(!rt.manager().unwrap().live().contains(user), "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn the_live_plan_counts_a_service_the_boot_took_down_when_its_consumer_was_refused() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // module 0 needs `local-ping` and its file is damaged after the plan: the service starts, the module is refused, and the orphaned service goes down again
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &LOCAL_PING_USER_B], &[LOCAL_PING]);
            let file = l.m.files.iter_mut().find(|(n, _)| n.starts_with(b"modules/local-ping-user.dkm")).unwrap();
            file.1[0] ^= 0xff;
            let mut rec = boot_record(&[LOCAL_PING_USER.leaf]);
            rec.count = 1;
            crate::boot::info().count = 1;
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            assert_ne!(crate::boot::info().results[0], dkrt_sdk::OK, "{}: the module is refused", dev.name);
            assert!(rt.manager().unwrap().live().is_empty(), "{}: the service went down with its only consumer", dev.name);
            // the live plan of a module that needs the same service: the service is down but nothing refused it, so a load can bring it up
            let mut caps = capacity(&l.p);
            crate::package::loader::live_caps(&mut caps, (0, 0));
            let table = l.tables[1][..l.table_len[1]].to_vec();
            assert_eq!(l.live_plan(1, &table, &caps), dkrt_sdk::OK, "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_refused_live_load_leaves_no_module_in_the_live_plan_and_frees_its_index() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER, &LOCAL_PING_USER_B], &[LOCAL_PING]);
            let mut rec = boot_record(&[LOCAL_PING_USER.leaf]);
            rec.count = 1;
            crate::boot::info().count = 1;
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            // the second module's file is damaged after the plan: its load is refused, as the old live load refused it, and index 1 is free again
            let path_b = std::format!("modules/{}.dkm\0", LOCAL_PING_USER_B.leaf).into_bytes();
            let file = l.m.files.iter_mut().find(|(n, _)| n.starts_with(b"modules/local-ping-user-b.dkm")).unwrap();
            file.1[0] ^= 0xff;
            let r = rt.live_load_with(1, path_b.as_ptr(), &live_cap(&l));
            assert_ne!(r, dkrt_sdk::OK, "{}: the damaged file is refused", dev.name);
            assert!(!l.accepted[1], "{}: a refused live load leaves no phantom module in the live plan", dev.name);
            assert_eq!(records::module(1).used, 0, "{}", dev.name);
            assert_eq!(l.sel_of_rec[1], 0, "{}: no free record keeps the index", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_module_the_boot_refused_for_a_missing_service_loads_live_once_the_service_is_there() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the service's file is damaged after the plan: the boot refuses its consumer (the plan's verdict says E_MISSING after the settle)
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            let file = l.m.files.iter_mut().find(|(n, _)| n.starts_with(b"services/local-ping")).unwrap();
            file.1[0] ^= 0xff;
            let rec = boot_record(&[LOCAL_PING_USER.leaf]);
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().results[0], dkrt_sdk::E_MISSING, "{}", dev.name);
            // the service is installed (the file is whole again): the boot's verdicts are over, so the live load goes through
            let file = l.m.files.iter_mut().find(|(n, _)| n.starts_with(b"services/local-ping")).unwrap();
            file.1[0] ^= 0xff;
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            assert_eq!(rt.live_load_with(0, path.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}", dev.name);
            assert!(rt.manager().unwrap().live().contains(id(0)) && rt.manager().unwrap().live().contains(id(MAX_MODULES)), "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_live_load_refused_before_anything_is_placed_logs_its_own_detail_and_not_that_of_an_earlier_refusal() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // module 1 needs `local-ping`, which the search did not find: the graph refuses it before any step runs
            let (mut l, _plan) = planned(dev, &image, &[], &[&SVC_USER, &LOCAL_PING_USER], &[SVC_TEST]);
            let mut rec = boot_record(&[SVC_USER.leaf]);
            rec.count = 1;
            crate::boot::info().count = 1;
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            // what an earlier refusal of another package left in the loader
            l.missing = [7; dkrt_sdk::NEEDS_LEN];
            l.decl_diff = (1, 2);
            l.import_hash = 5;
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            assert_eq!(rt.live_load_with(1, path.as_ptr(), &live_cap(&l)), dkrt_sdk::E_MISSING, "{}", dev.name);
            assert_eq!((crate::package::loader::missing(), l.decl_diff, l.import_hash), ([0; dkrt_sdk::NEEDS_LEN], (0, 0), 0), "{}: the refusal logs its own detail", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_live_load_finds_a_packed_service_on_the_card_that_the_boot_selection_did_not_need() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the card holds `local-ping` and its user, but the selection is `svc-test-user`, whose boot search looks for `svc-test` only
            let (mut l, _plan) = planned(dev, &image, &[], &[&SVC_USER, &LOCAL_PING_USER], &[SVC_TEST, LOCAL_PING]);
            let mut rec = boot_record(&[SVC_USER.leaf]);
            rec.count = 1;
            crate::boot::info().count = 1;
            let mut rt = Rt::new(&mut l as *mut _);
            rt.boot_with(&rec, &capacity(&l.p));
            let path = std::format!("modules/{}.dkm\0", LOCAL_PING_USER.leaf).into_bytes();
            assert_eq!(l.live_check_load(1, path.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}: the check finds the service", dev.name);
            assert_eq!(rt.live_load_with(1, path.as_ptr(), &live_cap(&l)), dkrt_sdk::OK, "{}", dev.name);
            assert!(rt.manager().unwrap().live().contains(id(1)), "{}: the module is up", dev.name);
            assert!(l.found.n >= 2, "{}: the provider was added to the boot's", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_selected_file_that_is_not_on_the_card_is_reported_and_the_rest_of_the_selection_loads() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        for gone_first in [false, true] {
            unsafe {
                let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
                let leaves: [&str; 2] = if gone_first { ["gone", LOCAL_PING_USER.leaf] } else { [LOCAL_PING_USER.leaf, "gone"] };
                let (live_k, gone_k) = if gone_first { (1, 0) } else { (0, 1) };
                if gone_first {
                    // the captured table of the module is at index 0: move it to its place
                    let t = (l.tables[0], l.table_len[0]);
                    l.tables[1] = t.0;
                    l.table_len[1] = t.1;
                    l.tables[0] = [0; TABLE_MAX];
                    l.table_len[0] = 0;
                }
                let rec = boot_record(&leaves);
                let mut rt = Box::new(Rt::new(&mut l as *mut _));
                rt.boot_with(&rec, &capacity(&l.p));
                let b = crate::boot::info();
                assert_eq!(b.results[gone_k], dkrt_sdk::E_NOT_FOUND, "{} gone_first={gone_first}: the missing file is reported", dev.name);
                assert_eq!(b.states[gone_k], 6u8, "{}: and has a state", dev.name);
                assert_eq!(b.results[live_k], dkrt_sdk::OK, "{} gone_first={gone_first}: the other module loads", dev.name);
            }
            clean_state();
        }
    }
}

/// The debug module with nothing else on the card (what a CLI delete of the last module leaves: no service, no port file) still loads: it needs no service and names no firmware item.
#[test]
fn the_debug_module_loads_from_a_card_with_no_service_and_no_port_file() {
    let _g = clean();
    const DEBUG: Fixture = Fixture { leaf: "debug", nx: "digikit/debug" };
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&DEBUG], &[]);
            l.m.files.retain(|(n, _)| !n.ends_with(b".dkport"));
            l.reset_plan();
            l.capture(0, &packed(dev, DEBUG.leaf, "dkm", DEBUG.nx));
            l.find_providers(1);
            let rec = boot_record(&[DEBUG.leaf]);
            let mut rt = Box::new(Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().results[0], dkrt_sdk::OK, "{}: {:x?}", dev.name, crate::boot::info().detail[0]);
        }
        clean_state();
    }
}

/// Two lfo shapes with one record key, through the runtime's boot (`Rt`, the boot record and the log as the device writes them): the first loads, the second is refused with
/// the plan's key status.
#[test]
fn of_two_lfo_shapes_with_one_key_the_first_loads_and_the_second_is_refused_by_the_boot() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LFO_SHAPE, &LFO_SHAPE], &SERVICES);
            let rec = boot_record(&[LFO_SHAPE.leaf, LFO_SHAPE.leaf]);
            let mut rt = Box::new(Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            let b = crate::boot::info();
            assert_eq!([b.results[0], b.results[1]], [dkrt_sdk::OK, dkrt_sdk::E_PLAN_KEY], "{}: detail {:x?}", dev.name, [b.detail[0], b.detail[1]]);
            assert_eq!(([b.states[0], b.states[1]], b.loaded), ([dkrt_sdk::STATE_ACTIVE, dkrt_sdk::STATE_REFUSED], 1), "{}", dev.name);
            // the detail words: the kind and the key (what the module states and the LOADS command show of the refusal)
            let key = Keys::parse(&table_of(&packed(dev, LFO_SHAPE.leaf, "dkm", LFO_SHAPE.nx))).unwrap().keys_of(dkneeds::KIND_LFO_SHAPE).next().expect("the fixture has a shape key");
            assert_eq!(b.detail[1], [dkneeds::KIND_LFO_SHAPE as u32, key], "{}", dev.name);
        }
        clean_state();
    }
}
