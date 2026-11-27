//! The task gate through the loader and the lifecycle's installers: the facts come from the names of the port file, the phase says whether the tasks run, a restore that
//! cannot be done yet answers `E_BUSY` and leaves the hook in.
use super::*;
use crate::package::lifecycle::RtManifest;
use dkneeds::keys::{name_hash_bytes, NameRec, FLAG_ABSENT, NAME_FW};
use super::seam_tests::{found_at, shared};
use dkrt_lifecycle::{InstallError, Linker, Manifest, Placer, ProviderHost};

const TCB_CONSOLE: u32 = 0x4039_be58;
const QUEUE: u32 = 0x403a_0eac;
const TCB_BG: u32 = 0x4317_2de4;
const CELL: u32 = 0x44f3_7090;
const OBJECT: u32 = 0x4500_0000;

fn name(n: &[u8], addr: u32) -> NameRec {
    NameRec { hash: name_hash_bytes(n), addr, n: 0, kind: NAME_FW, flags: 0 }
}

/// The four names of the tasks at the addresses of a device.
fn task_names() -> Vec<NameRec> {
    let t = park::TASK_NAMES;
    // TASK_NAMES: bgworker tcb, bgworker cell, console tcb, console queue
    let mut v = vec![name(t[0], TCB_BG), name(t[1], CELL), name(t[2], TCB_CONSOLE), name(t[3], QUEUE)];
    v.sort_by_key(|n| n.hash);
    v
}

fn put(l: &Fake, addr: u32, v: u32) {
    l.p.put_code(addr, &v.to_be_bytes());
}

/// Both tasks wait (each waiter word holds its TCB) or neither does.
fn park(l: &Fake, parked: bool) {
    put(l, CELL, OBJECT);
    put(l, OBJECT + 0x18, if parked { TCB_BG } else { 0 });
    put(l, QUEUE + 0xc, if parked { TCB_CONSOLE } else { 0 });
}

/// A started service (`hook.a`) whose port file lists the task names, the site with the flags `flags`.
unsafe fn started(flags: u8, names: &[NameRec]) -> (Fake, PackageId, u32) {
    let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
    put_port_names(&mut l, &[(("hook.a", SITE, STOCK.len() as u16, dkneeds::keys::SITE_RAW, check8(&STOCK)), flags)], names);
    (l, PackageId((MAX_MODULES + 1) as u8), 0)
}

fn site(l: &Fake) -> Vec<u8> {
    site_bytes(l, 6)
}

#[test]
fn at_run_time_a_task_hook_goes_in_only_while_the_tasks_wait_and_comes_out_the_same_way() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, svc, _) = started(0, &task_names());
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        // the loader is closed (the live job, not the boot): the tasks run
        l.set_phase(PhaseKind::Closed);
        park(&l, false);
        assert_eq!(installers.install(svc, manifest.entries(svc)), Err(InstallError::Backend(E_BUSY)), "a task runs: the install waits");
        assert_eq!((site(&l), l.installer.count()), (STOCK.to_vec(), 0));
        park(&l, true);
        installers.install(svc, manifest.entries(svc)).unwrap();
        assert_ne!(site(&l), STOCK.to_vec());
        // down: the BgWorker runs a job again, the entry stays and the answer is busy; the retry after it parked takes it out
        put(&l, OBJECT + 0x18, 0);
        assert_eq!(installers.restore(svc), Err(InstallError::Backend(E_BUSY)));
        assert_ne!(site(&l), STOCK.to_vec(), "the hook is still in");
        put(&l, OBJECT + 0x18, TCB_BG);
        installers.restore(svc).unwrap();
        assert_eq!((site(&l), l.installer.count()), (STOCK.to_vec(), 0));
    }
}

#[test]
fn at_the_boot_the_same_hook_goes_in_while_no_task_has_run() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, svc, _) = started(0, &task_names());
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        park(&l, false);
        l.set_phase(PhaseKind::Boot);
        installers.install(svc, manifest.entries(svc)).unwrap();
        assert_ne!(site(&l), STOCK.to_vec());
        installers.restore(svc).unwrap();
        assert_eq!(site(&l), STOCK.to_vec());
    }
}

#[test]
fn a_ui_hook_never_waits_for_a_task() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let flags = dkneeds::keys::CTX_UI << dkneeds::keys::FLAG_CTX_SHIFT;
        let (mut l, svc, _) = started(flags, &task_names());
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        l.set_phase(PhaseKind::Closed);
        park(&l, false);
        installers.install(svc, manifest.entries(svc)).unwrap();
        assert_eq!(installers.restore(svc), Ok(()));
        assert_eq!(site(&l), STOCK.to_vec());
    }
}

#[test]
fn the_facts_come_from_the_port_file_a_task_the_firmware_lacks_is_left_out_and_a_missing_name_makes_them_unknown() {
    use dkneeds::keys::{write_table, Keys, Pair, Table};
    let table = |names: &[NameRec]| {
        let t = Table { provides: &[Pair { a: 1, b: 2 }], fw: &[FW_ID], names, ..Default::default() };
        let mut b = vec![0u8; dkneeds::keys::table_size(&t)];
        write_table(&t, &mut b).unwrap();
        b
    };
    let facts = |names: &[NameRec]| {
        let b = table(names);
        let k = Keys::parse(&b).unwrap();
        let p = park::TaskParks::read(Some(&k));
        p.gate(true).parks.map(|x| x.to_vec())
    };
    let all = facts(&task_names()).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.contains(&install::Park { tcb: TCB_BG, wait: CELL, indirect: true, off: 0x18 }));
    assert!(all.contains(&install::Park { tcb: TCB_CONSOLE, wait: QUEUE, indirect: false, off: 0xc }));
    // a firmware without the console task: only the BgWorker is waited for
    let mut v = task_names();
    for n in v.iter_mut().filter(|n| n.hash == name_hash_bytes(park::TASK_NAMES[2]) || n.hash == name_hash_bytes(park::TASK_NAMES[3])) {
        n.flags = FLAG_ABSENT;
        n.addr = 0;
    }
    assert_eq!(facts(&v).unwrap().len(), 1);
    // a port file that lacks a name, or no port file: unknown
    v.retain(|n| n.hash != name_hash_bytes(park::TASK_NAMES[0]));
    assert_eq!(facts(&v), None);
    assert!(park::TaskParks::read(None).gate(true).parks.is_none());
    // the boot gate does not use them
    assert!(!park::TaskParks::read(None).gate(false).live);
}

#[test]
fn a_run_time_task_site_with_no_task_facts_is_e_import_and_the_load_names_the_cause() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a port file of an older build: the site is listed, the task names are not
        let (mut l, svc, _) = started(0, &[]);
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        l.set_phase(PhaseKind::Closed);
        assert_eq!(installers.install(svc, manifest.entries(svc)), Err(InstallError::Backend(E_IMPORT)));
        assert_eq!(l.missing(), *b"port:no-task", "the cause is in the failure detail the load logs");
        assert_eq!(site(&l), STOCK.to_vec());
    }
}
