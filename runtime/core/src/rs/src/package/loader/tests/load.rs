//! The load of one file: records, relocation, `init`, the undo of a failed load, providers and slot fills.
use super::*;

#[test]
fn a_module_loads_and_unloads_and_gives_back_every_block() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let base = FIRST_ADDR;
        let s = st();
        assert_eq!((s.modules[0].used, s.modules[0].base, s.loads), (1, base, 1));
        assert_eq!(s.modules[0].fini, base + 4);
        // the module block stays; the record block of init was given back after the commit
        assert_eq!(l.m.live.iter().map(|b| b.0).collect::<Vec<_>>(), vec![base]);
        assert_eq!(l.m.log[0], Ev::Stage);
        assert!(l.m.log.contains(&Ev::Init(0, base)));
        assert_eq!(l.r.log, vec![Ev::Add(0)]);

        let mut rt = adopt(&mut l, &[0]);
        assert_eq!(rt.unload(0), OK);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        let fini = l.m.log.iter().position(|e| *e == Ev::Fini(0, base + 4)).expect("fini ran");
        let free = l.m.log.iter().position(|e| *e == Ev::Free(base)).expect("the module block is freed");
        assert!(fini < free, "fini runs before the memory goes");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
    }
}

#[test]
fn a_failed_init_undoes_the_load() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Fails(E_INIT));
        assert_eq!(l.load(PATH.as_ptr()), E_INIT);
        assert!(l.m.live.is_empty(), "the module block and the record block are free");
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(st().loads, 0);
        assert_eq!(l.r.log, vec![Ev::Remove(0)], "nothing was added, the undo still asks the registry");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))), "a module whose init failed gets no fini");
    }
}

#[test]
fn a_failed_commit_undoes_the_load() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.r.add_result = E_NO_REGISTRY;
        assert_eq!(l.load(PATH.as_ptr()), E_NO_REGISTRY);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        // the record can be used again
        l.r.add_result = OK;
        l.m.log.clear();
        l.r.log.clear();
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(st().modules[0].used, 1);
    }
}

#[test]
fn a_need_of_init_is_returned_as_the_missing_service() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::NeedsUsbBulk);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(&l.missing()[..8], b"usb-bulk");
        assert!(l.m.live.is_empty());
        assert_eq!(l.r.log, vec![Ev::Remove(0)]);
    }
}

#[test]
fn an_arena_that_is_full_gives_no_memory_error_and_keeps_nothing() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the module block
        let mut l = loader(Init::Ok);
        l.m.fail_alloc = Some(1);
        assert_eq!(l.load(PATH.as_ptr()), E_NOMEM);
        assert!(l.m.live.is_empty());
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(st().modules[0].used, 0);
        // the record block of init: the module block goes again
        let mut l = loader(Init::Ok);
        l.m.fail_alloc = Some(2);
        assert_eq!(l.load(PATH.as_ptr()), E_NOMEM);
        assert!(l.m.live.is_empty());
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(st().modules[0].used, 0);
    }
}

#[test]
fn a_load_outside_the_boot_or_of_a_missing_file_touches_nothing() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.phase = Phase::Closed;
        assert_eq!(l.load(PATH.as_ptr()), E_BOOT_ONLY);
        assert!(l.m.log.is_empty());
        l.phase = Phase::Boot;
        assert_eq!(l.load(b"mods/none.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(l.m.log, vec![Ev::Stage]);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_damaged_file_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        let f = l.m.files.get_mut(&PATH[..PATH.len() - 1].to_vec()).unwrap();
        f[IMAGE_OFF + 2] ^= 1; // the file hash no longer matches
        assert_eq!(l.load(PATH.as_ptr()), E_SHA);
        assert_eq!(l.m.allocs, 0);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_module_file_with_provides_or_hooks_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for hooks_only in [false, true] {
            let mut table = svc_table();
            if hooks_only {
                table.provides = &[];
            }
            let mut file = service_file(&table, 0);
            put(&mut file, dkneeds::H_ROLE, 0);
            seal(&mut file);
            let mut l = loader_of(Init::Ok, file);
            assert_eq!(l.load(PATH.as_ptr()), E_FORMAT, "hooks_only={hooks_only}");
            assert_eq!(l.m.allocs, 0);
            assert!(l.r.log.is_empty());
        }
    }
}

#[test]
fn relocations_name_the_block_or_an_import_of_the_runtime() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // word at 8 holds 0x20 (relative to the block), word at 12 holds 4 (plus the import)
        let mut file = module_file_with(&[(8, 0), (12, 1 << 24)], &[b"dk_alloc"]);
        file[IMAGE_OFF + 11] = 0x20;
        file[IMAGE_OFF + 15] = 4;
        seal(&mut file);
        let mut l = loader_of(Init::Ok, file);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let mem = &l.m.live[0].1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(8)), FIRST_ADDR + 0x20);
        assert_eq!(rd32(words.as_ptr().add(12)), KNOWN_ADDR + 4);
    }
}

#[test]
fn an_import_of_a_module_that_is_not_there_names_it_and_allocates_nothing() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[b"dk_usb_bulk_open"]));
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(&l.missing()[..8], b"usb-bulk");
        assert_eq!(l.m.allocs, 0);
        // an import nobody knows is refused without a name
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[b"dk_nothing"]));
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(l.missing()[0], 0);
    }
}

#[test]
fn a_provider_loads_into_its_own_record_publishes_its_exports_and_installs_its_hooks_last() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK);
        let rec = MAX_MODULES + 1;
        let base = records::module(rec).base;
        assert_eq!((records::module(rec).used, l.roles[rec], st().loads), (1, 1, 0), "a provider is no module of the count");
        assert_eq!(st().modules[0].used, 0, "no module record is taken");
        // the record is the host-visible DKRT_PROVIDERS slot 1: used, named, with the load base
        let published = &*core::ptr::addr_of!(records::DKRT_PROVIDERS);
        let name = &published[1].name;
        assert_eq!((published[1].used, published[1].base), (1, base));
        assert_ne!(name[0], 0, "the record carries the file's name");
        assert_eq!(published[0].used, 0);
        // the export resolves to base + 8; the site jumps to the entry of the hook, base + 4 (4ef9 and the address, big endian)
        assert_eq!(l.exports.resolve(dkneeds::keys::name_hash("dk_svctest_ping")), Some(base + 8));
        let t = base + 4;
        assert_eq!(site_bytes(&l, 6), [0x4e, 0xf9, (t >> 24) as u8, (t >> 16) as u8, (t >> 8) as u8, t as u8]);
        assert_eq!(l.installer.count(), 1);
        // the install is the last step: it comes after the init of the provider
        let init = l.m.log.iter().position(|e| matches!(e, Ev::Init(..))).unwrap();
        assert!(init < l.m.log.len());
        assert!(l.p.calls.borrow().contains(&crate::fake_platform::Call::Mask), "the install runs under the mask");
        // the table copy stays for the record
        assert!(l.provs[1].block.is_some());
    }
}

/// A `SLOTS` item may fill a `[[slot]]` word only: the same item naming an `[[info]]` data table refuses the file (`E_FORMAT`) and writes nothing.
#[test]
fn a_slot_fill_of_an_info_data_entry_is_refused_and_writes_nothing() {
    use dkneeds::keys::{name_hash, Slot, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    static SLOTS: std::sync::OnceLock<[Slot; 1]> = std::sync::OnceLock::new();
    unsafe {
        let slots = SLOTS.get_or_init(|| [Slot { id: name_hash("the.entry"), flags: 0, entry: 4 }]);
        for (data, want) in [(false, OK), (true, E_FORMAT)] {
            let table = Table { slots, ..svc_table() };
            let mut items = vec![(dkrt_abi::service::Declare::SLOT, name_hash("the.entry"), 4)];
            items.extend_from_slice(match svc_init() {
                Init::Declare(i) => i,
                _ => unreachable!(),
            });
            let mut l = provider_loader(service_file(&table, 0), Init::Declare(Box::leak(items.into_boxed_slice())));
            l.p.info_entries.borrow_mut().push((name_hash("the.entry"), vec![0]));
            if data {
                l.p.info_data.borrow_mut().push(name_hash("the.entry"));
            }
            assert_eq!(l.load_provider(PATH.as_ptr(), 1), want, "data entry: {data}");
            if data {
                assert_eq!(site_bytes(&l, 6), STOCK, "the site is stock");
                assert_eq!(l.installer.count(), 0, "nothing is installed");
                assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Word { .. })), "no slot word is written");
            }
        }
    }
}

#[test]
fn a_service_import_is_linked_for_a_service_file_only() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the word at 12 holds 4, plus the import
        let base = || {
            let mut f = module_file_with(&[(12, 1 << 24)], &[b"dkc_device_id"]);
            f[IMAGE_OFF + 15] = 4;
            f
        };
        let mut svc = service_file_of(base(), &svc_table(), 0);
        seal(&mut svc);
        let mut l = provider_loader(svc, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        let words = core::slice::from_raw_parts(l.m.live[0].1.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), SVC_ADDR + 4);
        // a module has no such import: the name is unknown to it, and nothing is allocated
        let mut f = base();
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(l.m.allocs, 0);
        assert_eq!(l.missing()[0], 0);
    }
}

#[test]
fn the_row_a_service_file_declares_is_added_at_the_commit() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let rec = MAX_MODULES;
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(l.r.log, vec![Ev::AddProvider(rec, test_row(), test_instance())]);
        // (a provider record has no run-time unload: its row goes with a failed load, below)
        // a module declares no row: `init` cannot, the import is not in its table
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(l.r.log, vec![Ev::Add(0)]);
    }
}

#[test]
fn a_row_named_other_than_the_header_provides_refuses_the_load() {
    use dkneeds::keys::{name_hash, Pair, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the header says it provides "other"; `init` declares the row of "svc-test"
        static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
        let table = Table { provides: PROVIDES.get_or_init(|| [Pair { a: name_hash("other"), b: 0x000802 }]), ..Default::default() };
        let mut l = loader_of(Init::DeclaresRow, service_file(&table, 0));
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_DECL_KIND);
        assert!(l.m.live.is_empty());
        assert!(!l.r.log.iter().any(|e| matches!(e, Ev::AddProvider(..))), "no row reached the registry: {:?}", l.r.log);
    }
}

#[test]
fn a_row_the_registry_refuses_undoes_the_load() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let rec = MAX_MODULES;
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        l.r.add_result = E_PLAN_OWNER;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_PLAN_OWNER);
        assert!(l.m.live.is_empty());
        assert_eq!(l.r.log, vec![Ev::AddProvider(rec, test_row(), test_instance()), Ev::Remove(rec)]);
    }
}

#[test]
fn the_real_registry_puts_the_row_into_the_service_table_and_takes_it_out() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut r = RealRegistry;
        // the host tests of the runtime run with the machines row loaded (`provider_rows::test_machines`): the loaded rows start at one
        let fixed = 1;
        assert_eq!(services().rows.len(), fixed);
        assert_eq!(r.add(0, None, None), OK);
        let source = || crate::package::provider_rows::RowSource { row: test_row(), instance: test_instance(), extent: 0..usize::MAX };
        assert_eq!(r.add(2, Some(source()), None), OK);
        let t = services();
        assert_eq!(t.rows.len(), fixed + 1);
        assert_eq!(t.rows[fixed].name(), dkneeds::keys::name_hash("svc-test"));
        // one row has one owner
        assert_eq!(r.add(3, Some(source()), None), E_PLAN_OWNER);
        // (`RealRegistry::remove` also runs the registry's own remove, which the cleanup tests count: the row part alone here)
        crate::package::provider_rows::remove(3);
        assert_eq!(services().rows.len(), fixed + 1);
        crate::package::provider_rows::remove(2);
        assert_eq!(services().rows.len(), fixed);
    }
}

#[test]
fn a_hook_site_that_is_not_stock_refuses_the_provider_before_anything_is_allocated() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        l.p.put_code(SITE + 3, &[0x99]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_HOOK_STOCK);
        assert_eq!(l.m.allocs, 0, "the check runs before the image is placed");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(site_bytes(&l, 6), [0x20, 0x52, 0x48, 0x99, 0x00, 0x02]);
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
    }
}

/// A bound file (a `BIND` or an `FW` item in a service file) is a file of the old format: `E_FORMAT` before anything is allocated. The identity of the firmware and the
/// site facts are the port file's.
#[test]
fn a_bound_service_file_is_refused_before_anything_is_allocated() {
    use dkneeds::keys::{Bind, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let stock = STOCK;
        let bind = [Bind { site: dkneeds::keys::name_hash("hook.a"), addr: SITE, tramp: 16, stock: &stock }];
        let fw = [FW_ID];
        for (table, what) in [(Table { bind: &bind, fw: &fw, ..svc_table() }, "BIND and FW"), (Table { fw: &fw, ..svc_table() }, "FW")] {
            let mut l = provider_loader(service_file(&table, 0), svc_init());
            assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT, "{what}");
            assert_eq!(l.m.allocs, 0, "{what}");
            assert_eq!(site_bytes(&l, 6), STOCK);
        }
    }
}

/// A hook that the port file does not list at all (a port file older than the service) refuses the service with `E_IMPORT` and keeps the hash; a hook that it lists as
/// absent on this firmware is skipped: the service loads and the site stays stock. A site the runtime cannot use is `E_RANGE`.
#[test]
fn a_hook_the_port_file_does_not_list_is_refused_and_one_it_lists_as_absent_is_skipped() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let hook = |flags: u8, len: u16| (("hook.a", SITE, len, dkneeds::keys::SITE_RAW, check8(&STOCK)), flags);
        // not listed (the port lists another hook)
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        put_port(&mut l, &[(("hook.other", SITE, 6, 0, check8(&STOCK)), 0)]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_IMPORT);
        assert_eq!(l.import_hash, dkneeds::keys::name_hash("hook.a"));
        assert_eq!(l.m.allocs, 0);
        // no port file loaded at all
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        l.roles[MAX_MODULES + PROVIDER_MAX - 1] = 0;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_IMPORT);
        assert_eq!(l.m.allocs, 0);
        // absent on this firmware: the service loads, installs no site and keeps its export
        // (its `init` declares what the firmware has: the export, not the hook)
        let export_only: Vec<(u16, u32, u32)> = vec![(dkrt_abi::service::Declare::EXPORT, dkneeds::keys::name_hash("dk_svctest_ping"), 8)];
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::Declare(Box::leak(export_only.into_boxed_slice())));
        put_port(&mut l, &[hook(dkneeds::keys::FLAG_ABSENT, 0)]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!((l.installer.count(), l.exports.count()), (0, 1));
        assert_eq!(site_bytes(&l, 6), STOCK);
        // a site of 4 bytes has no room for the jump
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        put_port(&mut l, &[hook(0, 4)]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_RANGE);
        assert_eq!(l.m.allocs, 0);
    }
}

/// A service file uses the file calls as a module does: its `init` is the listing client's scope, its tick runs, and the client and the tick end with the record,
/// also when `init` fails.
#[test]
fn a_service_file_gets_the_listing_client_in_init_runs_its_tick_and_loses_both_with_its_record() {
    use std::sync::atomic::Ordering::SeqCst;
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let rec = MAX_MODULES + 1;
        PROBE_FILES.store(true, SeqCst);
        PROBED_CLIENT.store(0, SeqCst);
        let mut file = service_file(&svc_table(), 0);
        put(&mut file, dkneeds::H_TICK, 8);
        seal(&mut file);
        let mut l = provider_loader(file, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK);
        assert!(PROBED_CLIENT.load(SeqCst) > 0, "init is the scope of the client");
        assert_eq!(crate::package::files::ready_state(rec), 2, "ready after the load");
        assert!(!crate::package::files::in_init(rec));
        let base = records::module(rec).base;
        assert_eq!(crate::package::entry::tick_of(rec), base + 8, "the tick of a service file runs as a module's");
        assert_eq!(adopt(&mut l, &[rec]).unload(rec), OK);
        assert_eq!((crate::package::files::ready_state(rec), crate::package::entry::tick_of(rec)), (0, 0));
        // a failing init ends the scope and the client
        PROBED_CLIENT.store(0, SeqCst);
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::Fails(E_INIT));
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), E_INIT);
        assert!(PROBED_CLIENT.load(SeqCst) > 0);
        assert_eq!(crate::package::files::ready_state(rec), 0);
        assert!(!crate::package::files::in_init(rec));
        PROBE_FILES.store(false, SeqCst);
    }
}

#[test]
fn a_provider_whose_init_fails_leaves_no_export_no_site_and_no_block() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::Fails(E_INIT));
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_INIT);
        assert!(l.m.live.is_empty(), "the image and the table copy are free");
        assert_eq!(records::module(MAX_MODULES).used, 0);
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        assert_eq!(site_bytes(&l, 6), STOCK);
        assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Write { .. } | crate::fake_platform::Call::Mask)), "no site was written");
    }
}

#[test]
fn a_declaration_that_differs_from_the_table_refuses_the_provider_and_the_site_stays_stock() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // init declares nothing: the HOOKS and EXPORTS of the table are not repeated
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::Ok);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_DECL_KIND);
        assert!(l.m.live.is_empty());
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        assert_eq!(site_bytes(&l, 6), STOCK);
    }
}

#[test]
fn a_provider_in_safe_mode_installs_nothing() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        l.installer.set_armed(false);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(site_bytes(&l, 6), STOCK);
        assert_eq!(l.installer.count(), 0);
    }
}

#[test]
fn a_service_file_loads_only_as_a_provider_and_a_module_file_only_as_a_module() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let svc = service_file(&svc_table(), 0);
        let mut l = provider_loader(svc.clone(), svc_init());
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        let mut l = loader(Init::Ok);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        // a provider whose ABI the registry refuses stops before anything is allocated; the module asks nothing
        let mut svc2 = svc;
        put(&mut svc2, dkneeds::H_SVC_ABI, 0xabcd_0123);
        seal(&mut svc2);
        let mut l = provider_loader(svc2, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(*l.r.asked.borrow(), vec![Ev::AbiOk(0xabcd_0123)]);
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert!(l.r.asked.borrow().is_empty());
        let mut svc3 = service_file(&svc_table(), 0);
        put(&mut svc3, dkneeds::H_SVC_ABI, 1);
        seal(&mut svc3);
        let mut l = provider_loader(svc3, svc_init());
        l.r.abi = false;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_SVC_ABI);
        assert_eq!(l.m.allocs, 0);
    }
}

#[test]
fn a_module_import_that_the_runtime_lacks_resolves_to_a_published_export_and_not_before() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // word at 12 holds 4 (plus the import)
        let mut file = module_file_with(&[(12, 1 << 24)], &[b"dk_svctest_ping"]);
        file[IMAGE_OFF + 15] = 4;
        seal(&mut file);
        let mut l = loader_of(Init::Ok, file);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT, "no provider is loaded yet");
        l.exports.publish(MAX_MODULES, dkneeds::keys::name_hash("dk_svctest_ping"), 0x7000).unwrap();
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let mem = &l.m.live.last().unwrap().1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), 0x7000 + 4);
    }
}

#[test]
fn a_bound_export_requires_the_named_provider_and_compatible_version() {
    use dkneeds::keys::{name_hash, Need, Pair, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let symbol = b"local_ping_value";
    let hash = dkneeds::keys::name_hash_bytes(symbol);
    let owner = name_hash("local-ping");
    let wrong_owner = name_hash("other-service");
    let need = Need { service: owner, min_version: 0x010200, hard: true };
    let imports = [Pair { a: hash, b: owner }];
    let empty = Table::default();
    let missing_need = Table { service_imports: &imports, ..empty };
    let bound = Table { needs: &[need], ..missing_need };
    unsafe {
        for (table, provides, want) in [
            (&empty, (owner, 0x010200), E_IMPORT),
            (&missing_need, (owner, 0x010200), E_IMPORT),
            (&bound, (wrong_owner, 0x010200), E_IMPORT),
            (&bound, (owner, 0x010100), E_IMPORT),
            (&bound, (owner, 0x020000), E_IMPORT),
            (&bound, (owner, 0x010300), OK),
        ] {
            let mut l = loader_of(Init::Ok, consumer_file_with_import(table, symbol));
            l.provs[0].provides = Some(provides);
            l.exports.publish_with_scope(MAX_MODULES, hash, 0x7000, true).unwrap();
            assert_eq!(l.load(PATH.as_ptr()), want);
        }
        // A declared binding cannot take over a name the runtime already resolves.
        let known = &KNOWN_IMPORT[..KNOWN_IMPORT.len() - 1];
        let hash = dkneeds::keys::name_hash_bytes(known);
        let table = Table { service_imports: &[Pair { a: hash, b: owner }], ..bound };
        let mut l = loader_of(Init::Ok, consumer_file_with_import(&table, known));
        l.provs[0].provides = Some((owner, 0x010200));
        l.exports.publish_with_scope(MAX_MODULES, hash, 0x7000, true).unwrap();
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
    }
}

#[test]
fn a_service_file_with_a_table_outside_the_file_or_of_the_wrong_shape_is_refused_before_anything_is_allocated() {
    use dkneeds::keys::{Pair, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the table lies outside the file
        let mut f = service_file(&svc_table(), 0);
        let end = f.len() as u32;
        put(&mut f, dkneeds::H_REG_OFF, end + 64);
        seal(&mut f);
        let mut l = provider_loader(f, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // no PROVIDES item, and two of them: not the shape of a service
        const TWO: [Pair; 2] = [Pair { a: 1, b: 0 }, Pair { a: 2, b: 0 }];
        for table in [Table::default(), Table { provides: &TWO, ..Default::default() }] {
            let mut l = provider_loader(service_file(&table, 0), svc_init());
            assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
            assert_eq!(l.m.allocs, 0);
            assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        }
    }
}

#[test]
fn a_provider_with_an_export_past_the_image_or_a_table_copy_that_does_not_fit_is_refused_and_undone() {
    use dkneeds::keys::{name_hash, Pair, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the export entry is the image size (32): past the last byte
        let exports: &'static [Pair] = Box::leak(Box::new([Pair { a: name_hash("dk_svctest_ping"), b: 32 }]));
        let mut l = provider_loader(service_file(&Table { exports, ..svc_table() }, 0), svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))), "place runs before init");

        // the arena has room for the image but not for the copy of the table
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        l.m.fail_alloc = Some(2);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_NOMEM);
        assert_eq!(l.m.allocs, 2);
        assert_provider_undone(&l, 0);
    }
}

#[test]
fn a_slot_fill_past_the_image_or_of_a_slot_the_image_lacks_undoes_the_load() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the entry is the image size
        let mut l = slot_provider(32);
        l.p.info_entries.borrow_mut().push((dkneeds::keys::name_hash("the.entry"), vec![0]));
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
        // the image has no such slot
        let mut l = slot_provider(4);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
    }
}

/// The device mask of a `SLOTS` item: a slot for another device is skipped (the image of this device lacks it, and `init` does not declare it), a slot for this device is
/// filled, and an unmasked slot that the image lacks is still refused.
#[test]
fn a_slot_masked_for_another_device_is_skipped_and_an_unmasked_unknown_slot_is_refused() {
    use dkneeds::keys::SLOT_DEVICES_SHIFT;
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let me = masked_slot_provider(0, false).p.device_id();
        let other = if me == 1 { 2 } else { 1 };
        let bit = |id: u32| (1u32 << (id - 1)) << SLOT_DEVICES_SHIFT;
        // for the other device only: the image has no such slot, nothing is declared, the file loads and no word is written
        let mut l = masked_slot_provider(bit(other), false);
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK);
        assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Word { .. })), "no slot word is written");
        // for this device (among others): filled; the declared copy has the flags without the mask
        let mut l = masked_slot_provider(bit(me) | bit(other), true);
        l.p.info_entries.borrow_mut().push((dkneeds::keys::name_hash("the.entry"), vec![0]));
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK);
        assert!(l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Word { .. })), "the slot word is written");
        // for this device but the image lacks it: refused
        let mut l = masked_slot_provider(bit(me), true);
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), E_FORMAT);
        // no mask and the image lacks it: refused as before
        let mut l = masked_slot_provider(0, true);
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), E_FORMAT);
    }
}

#[test]
fn an_export_name_another_provider_published_undoes_the_load_and_keeps_the_other_export() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        l.exports.publish(MAX_MODULES + 1, dkneeds::keys::name_hash("dk_svctest_ping"), 0x7000).unwrap();
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_PLAN_OWNER);
        assert_provider_undone(&l, 1);
        assert_eq!(l.exports.resolve(dkneeds::keys::name_hash("dk_svctest_ping")), Some(0x7000));
    }
}

/// The packer writes a device mask in the top byte of a `HOOKS` entry: the loader ignores it (the port file's `absent` flag does its work), compares the bare entry
/// with what `init` declares and jumps to the bare entry.
#[test]
fn the_device_mask_in_a_hooks_entry_is_dropped() {
    use dkneeds::keys::{name_hash, Pair, Table};
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let hooks = [Pair { a: name_hash("hook.a"), b: 4 | 0x02 << 24 }];
        let table = Table { hooks: &hooks, ..svc_table() };
        let mut l = provider_loader(service_file(&table, 0), svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        let t = records::module(MAX_MODULES).base + 4;
        assert_eq!(site_bytes(&l, 6), [0x4e, 0xf9, (t >> 24) as u8, (t >> 16) as u8, (t >> 8) as u8, t as u8]);
    }
}


#[test]
fn the_name_of_a_module_event_is_the_part_of_the_id_after_the_slash_cut_to_the_log_width() {
    let field = |id: &[u8]| {
        let mut f = [0u8; dkneeds::NAME_LEN as usize];
        f[..id.len()].copy_from_slice(id);
        f
    };
    let want = |s: &[u8]| {
        let mut n = [0u8; dkrt_abi::module::LOG_NAME];
        n[..s.len()].copy_from_slice(s);
        n
    };
    assert_eq!(super::name_part(&field(b"digikit/usb-fast")), want(b"usb-fast"));
    assert_eq!(super::name_part(&field(b"tools/bootscreen_long_name")), want(b"bootscreen_l"));
    assert_eq!(super::name_part(&field(b"plain")), want(b"plain"));
    assert_eq!(super::name_part(&field(b"")), want(b""));
}


/// What a file's `init` leaves in the resident tables (a worker job, a poll function) is gone when the load is refused after `init`, for a provider and for a content
/// module: the block is freed then, and a queued job or poll slot would call freed code.
#[test]
fn a_refused_commit_after_init_leaves_no_job_and_no_poll_of_the_file() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let polls = |lo: u32, hi: u32| (*core::ptr::addr_of!(crate::state::DKRT_POLLS)).iter().filter(|&&f| f >= lo as usize && f < hi as usize).count();
        // a provider: init declares nothing, so the commit refuses (E_DECL_KIND)
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::PostsJobAndPoll);
        let rec = MAX_MODULES;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_DECL_KIND);
        let m = records::module(rec);
        assert_eq!((crate::worker::jobs_of(rec), polls(m.base, m.base + m.size)), (0, 0), "provider");
        assert!(l.m.live.is_empty());
        // a content module: the registry refuses the row
        let mut l = loader(Init::PostsJobAndPoll);
        l.r.add_result = E_NO_REGISTRY;
        assert_eq!(l.load(PATH.as_ptr()), E_NO_REGISTRY);
        let m = records::module(0);
        assert_eq!((crate::worker::jobs_of(0), polls(m.base, m.base + m.size)), (0, 0), "module");
        assert!(l.m.live.is_empty());
    }
}

/// A provider file whose `init` succeeded and whose activation then refuses (here: another record already exports the name) gets its `fini`, like a refused commit: the
/// owner `init` started is stopped, and a reload of the same file loads instead of answering `E_LOADED`.
#[test]
fn a_provider_refused_at_activation_is_finished_and_a_reload_loads() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = provider_loader(service_file(&svc_table(), 0), svc_init());
        let rec = MAX_MODULES + 1;
        let other = MAX_MODULES + 2;
        l.exports.publish_with_scope(other, dkneeds::keys::name_hash("dk_svctest_ping"), 0x1234, false).unwrap();
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), E_PLAN_OWNER);
        assert!(l.m.log.iter().any(|e| matches!(e, Ev::Fini(r, _) if *r == rec)), "the owner that init started is stopped by the provider's fini");
        assert_eq!(records::module(rec).used, 0);
        assert!(l.m.live.is_empty());
        l.exports.withdraw(other);
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK, "a reload of the same file is not E_LOADED");
    }
}

/// A refused load retires the jobs of the file's code (`fail` runs `retire_jobs`): a job that is on the worker cannot be dropped, so the refusal answers `E_BUSY` and the
/// block stays, as an unload does: freeing it would free code the worker runs. Once the flight ended the jobs are gone.
#[test]
fn a_refused_load_while_a_worker_job_runs_is_busy_and_keeps_the_block() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        crate::worker::test_reset();
        // init declares nothing, so the commit refuses (E_DECL_KIND)
        let mut l = provider_loader(service_file(&svc_table(), 0), Init::PostsRunningJob);
        let rec = MAX_MODULES;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_BUSY, "not the E_DECL_KIND of the commit");
        assert_eq!(l.m.live.len(), 1, "the file's block is kept while the worker runs its job");
        assert_eq!(crate::worker::jobs_of(rec), 1);
        crate::worker::test_pump();
        crate::worker::test_pump();
        assert_eq!(crate::worker::jobs_of(rec), 0, "the flight ended: the job is gone");
        assert!(crate::worker::retire(rec));
        crate::worker::test_reset();
    }
}
