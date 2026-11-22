//! The seams of the package manager (`lifecycle/`) over the loader fakes of `tests.rs`: `FakePlatform` for the code memory, `FakeImages` for the store and the
//! arena, `FakeRegistry`. Each seam has its cases: what it does for a package that fits and the refusal that is typical for it. The order across seams and the unwind of
//! a failed step are the manager's unit tests (`dkrt-lifecycle`); the seams over the real packed files are `lifecycle/manager_assembly_tests.rs`.
use super::*;
use crate::package::lifecycle::{RtManifest, Session};
use dkneeds::keys::{name_hash, name_hash_bytes, Need, Pair, Table};
use dkrt_abi::service::{Service, ServiceRow, ServiceTable};
use dkrt_lifecycle::{Cpu, Entry, Linker, Manifest, PackageManager, Placer, ProviderHost, SiteRef};

fn id(n: usize) -> PackageId {
    PackageId(n as u8)
}

/// Module 0 at `PATH`, and the shared state of the seams over `l`.
fn shared(l: &mut Fake) -> Session<FakePlatform, FakeImages, FakeRegistry> {
    let mut paths = [None; MAX_MODULES];
    paths[0] = Some(PATH.as_ptr());
    Session::new(l as *mut Fake, paths)
}

/// Provider `j` of the search is the file at `PATH` (the one file of the fake store).
fn found_at(l: &mut Fake, j: usize) {
    l.found.paths[j][..PATH.len() - 1].copy_from_slice(&PATH[..PATH.len() - 1]);
    l.found.path_len[j] = PATH.len() - 1;
    l.found.n = j + 1;
}

fn word(l: &Fake, block: usize, at: usize) -> u32 {
    let mem = &l.m.live[block].1;
    unsafe { rd32((mem.as_ptr() as *const u8).add(at)) }
}

/// A module of a word at 12 that holds 4 and relocates against the image (kind 0), and a word at 16 that names the import `dk_alloc` (the runtime's).
fn two_relocs() -> Vec<u8> {
    let mut f = module_file_with(&[(12, 0), (16, 1 << 24)], &[b"dk_alloc"]);
    f[IMAGE_OFF + 15] = 4;
    seal(&mut f);
    f
}

#[test]
fn the_placer_copies_the_image_and_relocates_it_against_the_base_only() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, two_relocs());
        let mut s = shared(&mut l);
        let (mut placer, mut linker, ..) = s.seams();
        let base = placer.place(id(0)).unwrap();
        let m = records::module(0);
        assert_eq!((m.used, m.base), (1, base), "the record is the package id");
        assert_eq!(word(&l, 0, 12), base + 4, "an image relocation is applied");
        assert_eq!(word(&l, 0, 16), 0, "an import relocation waits for the linker");
        linker.link(id(0), base, &[]).unwrap();
        assert_eq!(word(&l, 0, 16), KNOWN_ADDR, "the linker applies it");
        assert_eq!(word(&l, 0, 12), base + 4, "and leaves the image relocation as it was");
    }
}

#[test]
fn a_package_that_cannot_be_placed_takes_nothing_and_a_placed_one_is_freed() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a file whose DSP section does not check is refused (the section is read at the place, where its range is reserved), nothing is taken
        let mut dsp = module_file();
        put(&mut dsp, dkneeds::H_DSP_SIZE, 64);
        seal(&mut dsp);
        let mut l = loader_of(Init::Ok, dsp);
        let mut s = shared(&mut l);
        let (mut placer, ..) = s.seams();
        assert!(placer.place(id(0)).is_err());
        assert_eq!((l.m.allocs, records::module(0).used), (0, 0));
        // a package with no path (no module at the index) is not found
        assert_eq!(placer.place(id(1)), Err(E_NOT_FOUND));
        // a placed package is freed whole: the block and the record
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (mut placer, ..) = s.seams();
        placer.place(id(0)).unwrap();
        assert_eq!(l.m.live.len(), 1);
        placer.free(id(0));
        assert_eq!((l.m.live.len(), records::module(0).used), (0, 0));
        // a second free changes nothing
        placer.free(id(0));
        assert!(!s.retained(id(0)));
    }
}

fn consumer_needing(service_import: bool, symbol: &[u8]) -> Vec<u8> {
    let hash = name_hash_bytes(symbol);
    let owner = name_hash("local-ping");
    let need = Need { service: owner, min_version: 0x010200, hard: true };
    let imports = [Pair { a: hash, b: owner }];
    let table = if service_import { Table { needs: &[need], service_imports: &imports, ..Default::default() } } else { Table::default() };
    consumer_file_with_import(&table, symbol)
}

#[test]
fn the_linker_binds_an_import_to_an_export_of_a_service_of_the_needs_only() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let service = id(MAX_MODULES);
    let other = id(MAX_MODULES + 1);
    unsafe {
        for (bound, symbol) in [(true, &b"local_ping_value"[..]), (false, &b"dk_svctest_ping"[..])] {
            let mut l = loader_of(Init::Ok, consumer_needing(bound, symbol));
            let hash = name_hash_bytes(symbol);
            l.provs[0].provides = Some((name_hash("local-ping"), 0x010300));
            l.provs[1].provides = Some((name_hash("other"), 0x010000));
            l.exports.publish_with_scope(MAX_MODULES, hash, 0x7000, bound).unwrap();
            let mut s = shared(&mut l);
            let (mut placer, mut linker, ..) = s.seams();
            let base = placer.place(id(0)).unwrap();
            // the export belongs to a service that the package does not need
            assert_eq!(linker.link(id(0), base, &[other]), Err(E_IMPORT), "bound: {bound}");
            assert_eq!(word(&l, 0, 12), 4, "a refused link writes nothing");
            assert_eq!(linker.link(id(0), base, &[service]), Ok(()), "bound: {bound}");
            assert_eq!(word(&l, 0, 12), 0x7000 + 4);
            linker.unbind(id(0));
        }
    }
}

#[test]
fn a_module_starts_with_init_and_stops_with_fini_and_a_failed_start_is_freed() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (mut placer, mut linker, _, mut host) = s.seams();
        let base = placer.place(id(0)).unwrap();
        linker.link(id(0), base, &[]).unwrap();
        let d = host.start(id(0)).unwrap();
        assert_eq!((d.resources().len(), d.entries().len()), (0, 0), "a module declares nothing");
        assert!(l.m.log.contains(&Ev::Init(0, base)));
        assert_eq!(st().loads, 0, "the module is not live before its activation");
        host.activate(id(0)).unwrap();
        assert_eq!((st().loads, host.can_stop(id(0))), (1, Ok(())));
        host.deactivate(id(0));
        host.stop(id(0));
        assert!(l.m.log.contains(&Ev::Fini(0, base + 4)));
        placer.free(id(0));
        assert_eq!((l.m.live.len(), records::module(0).used), (0, 0));
        assert!(!s.retained(id(0)));
        // an `init` that fails undoes itself; the placed record is the placer's to free
        let mut l = loader_of(Init::Fails(E_INIT), module_file());
        let mut s = shared(&mut l);
        let (mut placer, mut linker, _, mut host) = s.seams();
        let base = placer.place(id(0)).unwrap();
        linker.link(id(0), base, &[]).unwrap();
        assert_eq!(host.start(id(0)).err(), Some(E_INIT));
        assert_eq!(records::module(0).used, 1);
        placer.free(id(0));
        assert_eq!((l.m.live.len(), records::module(0).used, st().loads), (0, 0, 0));
    }
}

struct Recorder;
impl<C: dkrt_abi::core_api::CoreApi> Service<C> for Recorder {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
    unsafe fn remove(_: &C, owner: u8) {
        REMOVED.lock().unwrap_or_else(|e| e.into_inner()).push(owner);
    }
    unsafe fn apply_record(_: &C, kind: u32, _body: *const u8, c: &dkrt_abi::service::ApplyCtx) -> i32 {
        APPLIED.lock().unwrap_or_else(|e| e.into_inner()).push((c.owner, kind));
        if kind == 99 {
            E_INIT
        } else {
            OK
        }
    }
}
static APPLIED: std::sync::Mutex<Vec<(u8, u32)>> = std::sync::Mutex::new(Vec::new());
static REMOVED: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
static RECORDER: ServiceRow = ServiceRow::of::<Recorder, crate::package::core_api::RealCore>("recorder", &[]);

#[test]
fn detach_calls_the_remove_of_the_rows_of_the_services_it_names_for_a_module_consumer() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (.., mut host) = s.seams();
        let service = MAX_MODULES + 2;
        assert_eq!(crate::package::svc_rows::add(service, &RECORDER, None), OK);
        REMOVED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        host.detach(id(3), &[id(service)]);
        // a service that is not named, a package without a row, and a consumer that is no module, call nothing
        host.detach(id(3), &[]);
        host.detach(id(3), &[id(service + 1)]);
        host.detach(id(MAX_MODULES), &[id(service)]);
        crate::package::svc_rows::remove(service);
        assert_eq!(*REMOVED.lock().unwrap_or_else(|e| e.into_inner()), [4], "the owner id is the record plus one, and only the rows of the named services are called");
    }
}

#[test]
fn a_service_starts_with_its_entries_and_the_installers_put_the_hook_in_and_back() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        found_at(&mut l, 1);
        let svc = id(MAX_MODULES + 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        // the entry: the hook site of the port file, at the entry of the hook in the placed image
        assert_eq!(manifest.entries(svc), [Entry::Hook { site: SiteRef::new(Cpu::ColdFire, SITE), target: base + 4 }]);
        assert_eq!(manifest.entries(id(0)), [], "the entries are those of the package that started last");
        assert_eq!(site_bytes(&l, 6), STOCK);
        assert_eq!(l.exports.resolve(name_hash("dk_svctest_ping")), None, "nothing is public after the start");
        installers.install(svc, manifest.entries(svc)).unwrap();
        let t = base + 4;
        assert_eq!(site_bytes(&l, 6), [0x4e, 0xf9, (t >> 24) as u8, (t >> 16) as u8, (t >> 8) as u8, t as u8]);
        assert_eq!(l.exports.resolve(name_hash("dk_svctest_ping")), None, "nor after the install");
        host.activate(svc).unwrap();
        assert_eq!(l.exports.resolve(name_hash("dk_svctest_ping")), Some(base + 8), "the export is published by the activation, after the entries are in");
        // down: the export goes, then the entry goes back before the service stops
        host.deactivate(svc);
        assert_eq!(l.exports.resolve(name_hash("dk_svctest_ping")), None);
        installers.restore(svc).unwrap();
        assert_eq!((site_bytes(&l, 6), l.installer.count()), (STOCK.to_vec(), 0));
        host.stop(svc);
        placer.free(svc);
        assert_eq!((records::module(svc.0 as usize).used, l.exports.count(), l.m.live.len()), (0, 0, 0));
        // a second restore is the same answer
        installers.restore(svc).unwrap();
        // restore of a package that never installed anything changes nothing
        installers.restore(id(MAX_MODULES + 3)).unwrap();
        assert_eq!((site_bytes(&l, 6), l.installer.count()), (STOCK.to_vec(), 0));
    }
}

#[test]
fn an_entry_outside_the_image_is_refused_by_the_file_checks_and_by_the_installers() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkneeds::keys::Pair;
        use dkrt_lifecycle::InstallError;
        // a service whose hook entry lies far behind its image
        let mut t = svc_table();
        let hooks = [Pair { a: name_hash("hook.a"), b: 0x7000 }];
        t.hooks = &hooks;
        let items: Vec<(u16, u32, u32)> =
            vec![(dkrt_abi::service::Declare::HOOK, name_hash("hook.a"), 0x7000), (dkrt_abi::service::Declare::EXPORT, name_hash("dk_svctest_ping"), 8)];
        let mut l = provider_loader(service_file(&t, 0), Init::Declare(Box::leak(items.into_boxed_slice())));
        found_at(&mut l, 1);
        let svc = id(MAX_MODULES + 1);
        let mut s = shared(&mut l);
        let (mut placer, ..) = s.seams();
        assert_eq!(placer.place(svc), Err(E_RANGE), "the file checks refuse a hook entry past the image");
        assert_eq!((site_bytes(&l, 6), l.m.live.len()), (STOCK.to_vec(), 0));
        // the installers refuse a target outside the placed image as well
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        let past = Entry::Hook { site: SiteRef::new(Cpu::ColdFire, SITE), target: base + 0x7000 };
        let before = Entry::Hook { site: SiteRef::new(Cpu::ColdFire, SITE), target: base - 4 };
        assert_eq!(installers.install(svc, &[past]), Err(InstallError::Backend(E_RANGE)));
        assert_eq!(installers.install(svc, &[before]), Err(InstallError::Backend(E_RANGE)));
        assert_eq!((site_bytes(&l, 6), l.installer.count()), (STOCK.to_vec(), 0), "a refusal writes nothing");
    }
}

#[test]
fn the_installers_take_all_or_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkrt_lifecycle::InstallError;
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        found_at(&mut l, 1);
        let svc = id(MAX_MODULES + 1);
        let mut s = shared(&mut l);
        let manifest = RtManifest::of(&mut s, 0);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let base = placer.place(svc).unwrap();
        linker.link(svc, base, &[]).unwrap();
        host.start(svc).unwrap();
        // a good hook and a hook the port file lacks: neither goes in
        let good = manifest.entries(svc)[0];
        let nowhere = Entry::Hook { site: SiteRef::new(Cpu::ColdFire, 0x1234_5678), target: base + 4 };
        assert_eq!(installers.install(svc, &[good, nowhere]), Err(InstallError::Backend(E_IMPORT)));
        assert_eq!((site_bytes(&l, 6), l.installer.count()), (STOCK.to_vec(), 0));
        installers.install(svc, &[good]).unwrap();
        assert_eq!(l.installer.count(), 1);
    }
}

#[test]
fn the_seams_leave_the_boot_index_as_they_found_it() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        found_at(&mut l, 1);
        let svc = id(MAX_MODULES + 1);
        let mut s = shared(&mut l);
        let (mut placer, mut linker, mut installers, mut host) = s.seams();
        let manifest = RtManifest::of(&mut s, 0);
        let idx = || core::ptr::read_volatile(core::ptr::addr_of!(crate::selection::BOOT_IDX));
        crate::selection::BOOT_IDX = 77;
        let base = placer.place(svc).unwrap();
        assert_eq!(idx(), 77, "place");
        linker.link(svc, base, &[]).unwrap();
        assert_eq!(idx(), 77, "link");
        host.start(svc).unwrap();
        assert_eq!(idx(), 77, "start");
        installers.install(svc, manifest.entries(svc)).unwrap();
        host.activate(svc).unwrap();
        assert_eq!(idx(), 77, "activate");
        host.deactivate(svc);
        installers.restore(svc).unwrap();
        host.stop(svc);
        placer.free(svc);
        assert_eq!(idx(), 77, "stop and free");
        crate::selection::BOOT_IDX = u32::MAX;
    }
}

#[test]
fn a_hook_the_port_file_lacks_refuses_the_entry_and_a_foreign_cpu_entry_is_the_other_backends() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        let mut s = shared(&mut l);
        let (_, _, mut installers, _) = s.seams();
        use dkrt_lifecycle::{InstallError, SlotRef};
        let nowhere = Entry::Hook { site: SiteRef::new(Cpu::ColdFire, 0x1234_5678), target: 0x7000 };
        assert_eq!(installers.install(id(MAX_MODULES + 1), &[nowhere]), Err(InstallError::Backend(E_IMPORT)));
        // an entry of the SHARC goes to the SHARC backend (the DSP slots of `DspInstaller`): it claims the slot and a restore gives it back, with the ColdFire installer untouched
        let sharc = Entry::Slot { slot: SlotRef::new(Cpu::Sharc, 1), target: 4 };
        assert_eq!(installers.install(id(MAX_MODULES + 2), &[sharc]), Ok(()));
        assert_eq!(l.installer.count(), 0);
        assert_eq!(installers.restore(id(MAX_MODULES + 2)), Ok(()));
    }
}

#[test]
fn the_manifest_holds_the_tables_of_the_modules_and_providers_and_the_apis_of_the_core() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, service_file(&svc_table(), 0));
        let module = module_file();
        l.capture(0, &module);
        let svc = service_file(&svc_table(), 0);
        let t = table_of_file(&svc).unwrap().to_vec();
        l.found.tables[1][..t.len()].copy_from_slice(&t);
        l.found.table_len[1] = t.len();
        found_at(&mut l, 1);
        let mut s = shared(&mut l);
        let m = RtManifest::of(&mut s, 2);
        let ids: Vec<u8> = m.tables().iter().map(|(p, _)| p.0).collect();
        // module 1 has no table (nothing was captured for it) and neither has provider 0: each is a node with the empty table, so the loader's own checks refuse its file
        // with their code and it meets no need
        assert_eq!(ids, [0, 1, MAX_MODULES as u8, MAX_MODULES as u8 + 1]);
        let empty = dkneeds::keys::Keys::parse(m.tables()[1].1).expect("the empty table parses");
        assert_eq!((empty.words(dkneeds::keys::tag::NEEDS, 0), empty.words(dkneeds::keys::tag::PROVIDES, 0)), (None, None));
        assert_eq!(m.tables()[2].1, m.tables()[1].1, "provider 0 has the empty table too");
        assert_eq!(m.tables()[3].1, &t[..]);
        assert!(m.core_apis().iter().any(|a| a.name() == name_hash("runtime") && a.version() == dkrt_sdk::RUNTIME_VERSION));
        assert!(m.resources(id(0)).is_empty() && m.wants(id(0)).is_empty() && m.core_resources().is_empty());
        // the graph the manager builds from it
        let (p, li, i, h) = s.seams();
        let pm = PackageManager::new(m, p, li, i, h).unwrap();
        assert!(pm.graph().node(id(0)).is_some() && pm.graph().node(id(MAX_MODULES + 1)).is_some());
    }
}

/// A section of records as the commit leaves it: the header (magic, version, device, count) and each record as kind, whole length and a body word.
fn registry_block(kinds: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    for w in [0x444b_5247u32, 1, 0, kinds.len() as u32] {
        b.extend_from_slice(&w.to_be_bytes());
    }
    for &k in kinds {
        for w in [k, 12, 0] {
            b.extend_from_slice(&w.to_be_bytes());
        }
    }
    b
}

/// A service that owns the kinds of one parity (`T` = 1: odd, 2: even) and logs each call with its tag; a record of kind 99 is refused.
struct Tag<const T: u32>;
impl<const T: u32, C: dkrt_abi::core_api::CoreApi> Service<C> for Tag<T> {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
    unsafe fn apply(c: &C, _: &dkrt_abi::service::ApplyCtx) -> i32 {
        WALKED.lock().unwrap_or_else(|e| e.into_inner()).push((T, 0));
        BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()).push(("apply", c.boot_index()));
        OK
    }
    unsafe fn apply_record(_: &C, kind: u32, _body: *const u8, _: &dkrt_abi::service::ApplyCtx) -> i32 {
        if kind % 2 != T % 2 {
            return OK;
        }
        WALKED.lock().unwrap_or_else(|e| e.into_inner()).push((T, kind));
        if kind == 99 {
            E_INIT
        } else {
            OK
        }
    }
    unsafe fn remove(c: &C, owner: u8) {
        WALKED.lock().unwrap_or_else(|e| e.into_inner()).push((T, 1000 + owner as u32));
        BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()).push(("remove", c.boot_index()));
    }
}
/// The selection index a `Tag` row read in each of its `apply` and `remove` calls.
static BOOT_SEEN: std::sync::Mutex<Vec<(&str, u32)>> = std::sync::Mutex::new(Vec::new());
static WALKED: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());
static ODD: ServiceRow = ServiceRow::of::<Tag<1>, crate::package::core_api::RealCore>("odd", &[(3, 12), (5, 12), (99, 12)]);
static EVEN: ServiceRow = ServiceRow::of::<Tag<2>, crate::package::core_api::RealCore>("even", &[(2, 12), (4, 12)]);

fn walked() -> Vec<(u32, u32)> {
    core::mem::take(&mut *WALKED.lock().unwrap_or_else(|e| e.into_inner()))
}

#[test]
fn the_rows_read_the_consumers_record_as_the_boot_index_in_apply_and_remove_during_attach_and_detach() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _ = crate::package::handlers::services();
        let machines = MAX_MODULES + dkrt_sdk::PROVIDER_MAX - 1;
        crate::package::svc_rows::remove(machines);
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (.., mut host) = s.seams();
        let (odd, even) = (MAX_MODULES, MAX_MODULES + 1);
        assert_eq!(crate::package::svc_rows::add(odd, &ODD, Some(10)), OK);
        assert_eq!(crate::package::svc_rows::add(even, &EVEN, Some(20)), OK);
        let block = registry_block(&[4, 3]);
        l.reg_block[3] = (block.as_ptr(), block.len() as u32);
        let idx = || core::ptr::read_volatile(core::ptr::addr_of!(crate::selection::BOOT_IDX));
        crate::selection::BOOT_IDX = u32::MAX;
        BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
        host.attach(id(3), &[id(odd), id(even)]).unwrap();
        assert_eq!(*BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()), [("apply", 3), ("apply", 3)], "apply reads the consumer's record");
        BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
        host.detach(id(3), &[id(odd), id(even)]);
        assert_eq!(*BOOT_SEEN.lock().unwrap_or_else(|e| e.into_inner()), [("remove", 3), ("remove", 3)], "remove reads the consumer's record");
        assert_eq!(idx(), u32::MAX, "the index is as it was");
        crate::package::svc_rows::remove(odd);
        crate::package::svc_rows::remove(even);
        crate::package::svc_rows::add(machines, crate::package::handlers::exports::machines_row(), None);
    }
}

#[test]
fn attach_walks_every_apply_then_the_records_in_file_order_across_the_services_as_the_old_apply_did_and_a_refusal_is_undone() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _ = crate::package::handlers::services(); // the first call adds the test row of the machines service, at the last provider record
        // the walk is over the rows of this test alone: the machines row takes the place of its record again at the end
        let machines = MAX_MODULES + dkrt_sdk::PROVIDER_MAX - 1;
        crate::package::svc_rows::remove(machines);
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (.., mut host) = s.seams();
        let (odd, even) = (MAX_MODULES, MAX_MODULES + 1);
        // the callback order: odd, then even; the records interleave as in the file (kinds of both services, the shared rows are numbered in this order)
        assert_eq!(crate::package::svc_rows::add(odd, &ODD, Some(10)), OK);
        assert_eq!(crate::package::svc_rows::add(even, &EVEN, Some(20)), OK);
        let block = registry_block(&[4, 3, 2, 5]);
        l.reg_block[3] = (block.as_ptr(), block.len() as u32);
        walked();
        // the old walk was over the whole table of services (the rows loaded from the files)
        let table = crate::package::handlers::services();
        assert_eq!(crate::package::registry::apply_with(table, 4, block.as_ptr(), block.len() as u32, 0), OK);
        let old = walked();
        assert_eq!(old, [(1, 0), (2, 0), (2, 4), (1, 3), (2, 2), (1, 5)], "the old walk: every apply, then the records in file order");
        // a consumer that needs every loaded service, in any order, gets the same walk
        host.attach(id(3), &[id(even), id(odd)]).unwrap();
        assert_eq!(walked(), old, "one attach over the services walks as the old registry apply did");
        // a service the module does not need is not called, and the records it owns are not applied
        let even_only = registry_block(&[4, 2]);
        l.reg_block[3] = (even_only.as_ptr(), even_only.len() as u32);
        host.attach(id(3), &[id(even)]).unwrap();
        assert_eq!(walked(), [(2, 0), (2, 4), (2, 2)]);
        // a record whose kind belongs to a loaded service that the module does not need is refused before anything is applied
        l.reg_block[3] = (block.as_ptr(), block.len() as u32);
        assert_eq!(host.attach(id(3), &[id(even)]), Err(E_MISSING));
        assert_eq!(walked(), []);
        // a service with no row owns no kind: the module's records are refused (and none is applied)
        assert_eq!(host.attach(id(3), &[id(even + 1)]), Err(E_MISSING));
        assert_eq!(walked(), []);
        // a module with no records and a consumer that is no module apply nothing
        host.attach(id(2), &[id(odd), id(even)]).unwrap();
        host.attach(id(MAX_MODULES), &[id(odd), id(even)]).unwrap();
        assert_eq!(walked(), []);
        // the first refusal stops the walk and is the answer; the walk then undid what it wrote (`remove` for the owner on the same rows, the last first), so no detach follows
        let bad = registry_block(&[3, 99, 4]);
        l.reg_block[3] = (bad.as_ptr(), bad.len() as u32);
        assert_eq!(host.attach(id(3), &[id(odd), id(even)]), Err(E_INIT));
        assert_eq!(walked(), [(1, 0), (2, 0), (1, 3), (1, 99), (2, 1004), (1, 1004)]);
        crate::package::svc_rows::remove(odd);
        crate::package::svc_rows::remove(even);
        crate::package::svc_rows::add(machines, crate::package::handlers::exports::machines_row(), None);
    }
}

/// A service that refuses the take-down of module owner 2 only (the module of record 1).
struct Refuser;
impl<C: dkrt_abi::core_api::CoreApi> Service<C> for Refuser {
    dkrt_abi::accepts_records!();
    unsafe fn teardown_refusal(_core: &C, owner: u8) -> i32 {
        if owner == 2 {
            E_RELOAD_ROUTED
        } else {
            OK
        }
    }
}
static REFUSER: ServiceRow = ServiceRow::of::<Refuser, crate::package::core_api::RealCore>("refuser", &[]);

#[test]
fn can_stop_of_a_module_asks_the_services_about_that_module_only_and_a_service_is_not_asked() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _ = crate::package::handlers::services();
        let rec = MAX_MODULES + 1;
        assert_eq!(crate::package::svc_rows::add(rec, &REFUSER, None), OK);
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        let (.., mut host) = s.seams();
        assert_eq!(host.can_stop(id(0)), Ok(()));
        assert_eq!(host.can_stop(id(1)), Err(E_RELOAD_ROUTED), "the module of owner 2");
        assert_eq!(host.can_stop(id(rec)), Ok(()), "a service is asked nothing: its consumers are");
        crate::package::svc_rows::remove(rec);
    }
}

#[test]
fn a_load_starts_with_no_detail_of_an_earlier_refusal() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut l = unsafe { loader_of(Init::Ok, module_file()) };
    l.missing = [7; NEEDS_LEN];
    l.decl_diff = (1, 2);
    l.import_hash = 5;
    l.last_name = [9; LOG_NAME];
    l.clear_failure_detail();
    let d = l.failure_detail();
    assert_eq!((d.missing, d.decl_diff, d.import_hash, d.name), ([0; NEEDS_LEN], (0, 0), 0, [0; LOG_NAME]));
}

#[test]
fn start_dsp_hands_a_package_with_a_dsp_part_to_the_dsp_loader_once_and_a_package_without_one_to_nobody() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        *crate::package::dsp::ds() = core::mem::zeroed();
        crate::package::dsp::init_once();
        let mut l = loader_of(Init::Ok, module_file());
        let mut s = shared(&mut l);
        s.dsp_wanted[0] = false;
        let (.., mut host) = s.seams();
        assert!(!host.start_dsp(id(0)), "no DSP part: nothing is queued");
        assert_eq!(crate::package::dsp::ds().phase, 0);
        drop(host);
        s.dsp_wanted[0] = true;
        let (.., mut host) = s.seams();
        assert!(host.start_dsp(id(0)));
        assert_eq!((crate::package::dsp::ds().pending.rec, crate::package::dsp::ds().pending.early), (0, 1), "the load of record 0 started, early (the CF part is live)");
        assert_ne!(crate::package::dsp::ds().phase, 0);
        assert!(!host.start_dsp(id(0)), "the part is handed over once");
        *crate::package::dsp::ds() = core::mem::zeroed();
    }
}
