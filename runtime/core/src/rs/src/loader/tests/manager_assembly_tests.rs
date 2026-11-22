//! The package manager over the runtime's real seams and the real packed `local-ping` fixtures (docs/plans/runtime-contract-and-lifecycle.md, RC2b-1): the service and
//! two consumers as `patchkit pack` leaves them, on `FakePlatform`, through the same checks, role steps and commits as the boot's load loop. What the host cannot show stays
//! out (no ColdFire code runs: `init` of the service replays its declarations, `init` of a module is module code). The cases: bring-up from the selection, the unload of
//! the first consumer (the service stays for the second), the unload of the last (the orphaned service follows), and the memory afterwards; and the hooks of `svc-test`
//! (a ColdFire entry): the installer holds them after the bring-up and the firmware bytes are stock after the unload.
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware image is skipped.
use super::assembly_tests::*;
use super::*;
use crate::lifecycle::{RtManifest, Session};
use dkrt_lifecycle::{PackageId, PackageManager};

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
            let manifest = RtManifest::of(&s, 2);
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
            assert!(crate::svc_rows::rows().iter().any(|row| (**row).name() == "local-ping"), "{}: its row is in the table", dev.name);
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [1, 1, 1]);
            assert_eq!(st().loads, 2, "two modules are loaded");
            // the consumer's import word is the export's address
            let export = l.exports.owned(hash).unwrap().0;
            let found_export = l.m.live.iter().any(|(_, mem)| {
                let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, mem.len() * 16);
                words.chunks_exact(4).any(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) == export)
            });
            assert!(found_export, "{}: a consumer's image holds the address of the service's export", dev.name);

            // the first consumer goes, the service stays for the second
            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user], "{}", dev.name);
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [0, 1, 1]);

            // the last consumer goes and the orphaned service follows
            let report = pm.unload(user_b).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user_b, service], "{}", dev.name);
            assert!(!s.retained(user) && !s.retained(user_b) && !s.retained(service));
            assert_eq!([st().modules[0].used, st().modules[1].used, records::module(MAX_MODULES).used], [0, 0, 0]);
            assert_eq!(l.exports.count(), 0, "the export is withdrawn");
            assert!(!crate::svc_rows::rows().iter().any(|row| (**row).name() == "local-ping"), "its row is gone");
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
            assert_eq!(l.installer.count(), 0);
            assert_eq!(*l.p.code.borrow(), stock, "{}: the firmware is as it was", dev.name);
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
            let manifest = RtManifest::of(&s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            // the services of the search in the order of `found`: svc-test, then the port file that its hooks name
            let (user, service, port) = (id(0), id(MAX_MODULES), id(MAX_MODULES + 1));
            crate::selection::BOOT_IDX = u32::MAX;

            let report = pm.boot(&[user]).unwrap();
            assert!(report.refused().is_empty(), "{}: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!(report.brought_up().as_slice(), [port, service, user], "{}", dev.name);
            assert_eq!(core::ptr::read_volatile(core::ptr::addr_of!(crate::selection::BOOT_IDX)), u32::MAX, "{}: the bring-up leaves the boot index as it was", dev.name);
            // the hooks of the service are in: the installer holds its sites and the firmware is not stock any more
            assert!(l.installer.count() > 0, "{}: the installer recorded the hooks of the test service", dev.name);
            assert_ne!(*l.p.code.borrow(), stock, "{}: the hook sites are patched", dev.name);

            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user, service, port], "{}", dev.name);
            assert_eq!(l.installer.count(), 0, "{}", dev.name);
            assert_eq!(*l.p.code.borrow(), stock, "{}: the firmware bytes are stock after the unload", dev.name);
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
            assert_eq!(core::ptr::read_volatile(core::ptr::addr_of!(crate::selection::BOOT_IDX)), u32::MAX, "{}: the unload leaves the boot index as it was", dev.name);
        }
        clean_state();
    }
}
