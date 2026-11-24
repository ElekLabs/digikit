//! The patch entries of a service over the runtime's real seams (docs/plans/runtime-contract-and-lifecycle.md, RC4-0): the packed fixture `local-patch` (a service that takes one
//! `[[patch]]`, two bytes of a dead size bound of the chooser's order vector copy) and its user, as `patchkit pack` leaves them, with the port file of the test image on `FakePlatform`.
//! What the host cannot show stays out: the service's `init` is replayed from its header table (the value is `PATCH_TEST_VALUE`), so the unit tests of `svc-rt` own the declaration
//! and `install.rs` owns the installer; here the one case that fits and the refusals of the seams: the stock check of the port file and the length of the site.
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware image is skipped.
use super::assembly_tests::*;
use super::*;
use crate::package::lifecycle::{RtManifest, Session};
use dkrt_lifecycle::{LifecycleError, PackageId, PackageManager};

/// The site of the patch on `dev`: its address and stock bytes, from the port file of the test image and the local firmware.
fn patch_site(dev: Dev) -> (u32, Vec<u8>) {
    stock_sites(dev).remove(&dkneeds::keys::name_hash("cf.filter.hook.test_bound_patch")).expect("the port set lists the test patch")
}

fn bytes_at(l: &Boot, addr: u32, n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    unsafe { l.p.read_code(addr, &mut out) };
    out
}

fn id(n: usize) -> PackageId {
    PackageId(n as u8)
}

#[test]
fn a_service_with_a_patch_installs_it_and_the_firmware_is_stock_after_the_unload() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        let (site, stock) = patch_site(dev);
        assert_eq!(stock, [0xff, 0xff], "{}: the site is the low word of the immediate of `cmpi.l #0x3fffffff,D1`", dev.name);
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PATCH_USER], &[LOCAL_PATCH]);
            assert_eq!(l.found.n, 2, "{}: the search finds the service and the port file its patch names", dev.name);
            let before = l.p.code.borrow().clone();
            let path = std::format!("modules/{}.dkm\0", LOCAL_PATCH_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let (user, service, port) = (id(0), id(MAX_MODULES), id(MAX_MODULES + 1));
            crate::package::boot_idx::set(u32::MAX);
            assert_eq!(bytes_at(&l, site, 2), stock);

            let report = pm.boot(&[user]).unwrap();
            assert!(report.refused().is_empty(), "{}: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!(report.brought_up().as_slice(), [port, service, user], "{}", dev.name);
            // the patch is in: the installer holds it, the site has the bytes `start` computed and nothing else of the firmware changed
            assert_eq!((l.installer.patch_count(), l.installer.count()), (1, 0), "{}", dev.name);
            assert_eq!(bytes_at(&l, site, 2), (PATCH_TEST_VALUE as u16).to_be_bytes(), "{}: the patch bytes are at the site", dev.name);
            let patched = l.p.code.borrow().clone();
            let changed: Vec<u32> = patched.iter().filter(|(a, b)| before.get(a) != Some(b)).map(|(a, _)| *a).collect();
            assert_eq!(changed, [site + 1], "{}: only the second byte of the site changed (0xff to 0xfe)", dev.name);

            let report = pm.unload(user).unwrap();
            assert_eq!(report.taken_down().as_slice(), [user, service, port], "{}", dev.name);
            assert_eq!(l.installer.patch_count(), 0, "{}", dev.name);
            assert_eq!(*l.p.code.borrow(), before, "{}: the firmware bytes are stock after the unload", dev.name);
            assert!(l.m.live.is_empty(), "{}: every block of the arena is back", dev.name);
        }
        clean_state();
    }
}

/// The status of the first refusal of a boot.
fn refused_status(report: &dkrt_lifecycle::Report) -> Vec<i32> {
    report
        .refused()
        .iter()
        .filter_map(|r| if let LifecycleError::Failed { status, .. } = r.reason { Some(status) } else { None })
        .collect()
}

#[test]
fn a_patch_site_that_is_not_stock_refuses_the_service_with_the_stock_status_and_writes_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        let (site, _) = patch_site(dev);
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PATCH_USER], &[LOCAL_PATCH]);
            // another firmware revision, or another patch, left other bytes at the site
            l.p.put_code(site, &[0xff, 0xfd]);
            let before = l.p.code.borrow().clone();
            let path = std::format!("modules/{}.dkm\0", LOCAL_PATCH_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let report = pm.boot(&[id(0)]).unwrap();
            assert!(!report.brought_up().as_slice().contains(&id(MAX_MODULES)), "{}: the service is not up", dev.name);
            assert_eq!(refused_status(&report), [dkrt_sdk::E_HOOK_STOCK], "{}: the refusal says the site is not stock: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!((l.installer.patch_count(), *l.p.code.borrow() == before), (0, true), "{}: nothing was written", dev.name);
            assert!(l.m.live.is_empty() || !crate::package::provider_rows::has("local-patch"), "{}: no row of the refused service stays", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_patch_declared_with_another_length_than_its_site_is_refused_and_leaves_the_site_stock() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        let (site, stock) = patch_site(dev);
        unsafe {
            let (mut l, _plan) = planned(dev, &image, &[], &[&LOCAL_PATCH_USER], &[LOCAL_PATCH]);
            l.m.skew_patch_len = true;
            let path = std::format!("modules/{}.dkm\0", LOCAL_PATCH_USER.leaf).into_bytes();
            let mut at = [None; MAX_MODULES];
            at[0] = Some(path.as_ptr());
            let mut s = Session::new(&mut l as *mut _, at);
            let manifest = RtManifest::of(&mut s, 1);
            let (placer, linker, installers, host) = s.seams();
            let mut pm = PackageManager::new(manifest, placer, linker, installers, host).unwrap();
            let report = pm.boot(&[id(0)]).unwrap();
            assert!(!report.brought_up().as_slice().contains(&id(MAX_MODULES)), "{}: the service is not up", dev.name);
            assert_eq!(refused_status(&report), [dkrt_sdk::E_FORMAT], "{}: {:?}", dev.name, report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!((l.installer.patch_count(), bytes_at(&l, site, 2)), (0, stock), "{}", dev.name);
        }
        clean_state();
    }
}
