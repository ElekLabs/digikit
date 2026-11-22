//! The boot plan: captured tables, provider search, admission and the keys the services number.
use super::*;

#[test]
fn a_provider_whose_table_does_not_parse_is_still_tried_and_never_meets_a_need() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = staged_boot(service_file(&svc_table(), 0), consumer_file(&user_table()));
        l.find_providers(1);
        assert_eq!(l.found.n, 1);
        // the table the search kept is torn: the shared parser refuses it, the file in the store is fine
        let len = l.found.table_len[0];
        l.found.tables[0][..len].fill(0xff);
        let _rt = boot_selection(&mut l, &[PATH]);
        // the provider is accepted as before and loads (its file is checked by the loader), but it provides nothing the graph knows: the module that needs it is refused
        assert_eq!(records::module(MAX_MODULES).used, 1, "the provider was tried");
        let hash = dkneeds::keys::name_hash(SVC);
        let b = crate::boot::info();
        assert_eq!((b.states[0], b.results[0], b.detail[0]), (STATE_REFUSED, E_MISSING, [hash, 0]));
        assert_eq!(records::module(0).used, 0, "the module was not placed");
    }
}

#[test]
fn the_boot_finds_the_provider_for_a_hard_need_loads_it_first_and_the_consumer_resolves_its_import() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = staged_boot(service_file(&svc_table(), 0), consumer_file(&user_table()));
        l.find_providers(1);
        assert_eq!(l.found.n, 1, "the search found the provider");
        assert_eq!(l.found.path(0), SVC_PATH);
        let _rt = boot_selection(&mut l, &[PATH]);
        let rec = MAX_MODULES;
        assert_eq!(records::module(rec).used, 1);
        assert_eq!((crate::boot::info().states[0], crate::boot::info().results[0]), (STATE_ACTIVE, OK));
        // the module stage: the import is the provider's export
        let mem = &l.m.live.last().unwrap().1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), records::module(rec).base + 8 + 4);
    }
}

#[test]
fn no_search_without_an_unmet_need_and_a_failed_provider_refuses_its_dependents_with_the_hash() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a selection with no need: no listing, no provider
        let mut l = staged_boot(service_file(&svc_table(), 0), module_file());
        let allocs = l.m.allocs;
        l.find_providers(1);
        assert_eq!((l.found.n, l.m.allocs), (0, allocs), "nothing was searched");
        // the provider's site is not stock: the provider fails, and its dependent is refused before the module stage, with the service's hash
        let mut l = staged_boot(service_file(&svc_table(), 0), consumer_file(&user_table()));
        l.p.put_code(SITE, &[0u8; 6]);
        l.find_providers(1);
        assert_eq!(l.found.n, 1);
        let _rt = boot_selection(&mut l, &[PATH]);
        assert_eq!(records::module(MAX_MODULES).used, 0);
        assert!(l.unusable[0], "a provider the boot refused is not counted by a live check");
        let hash = dkneeds::keys::name_hash(SVC);
        let b = crate::boot::info();
        assert_eq!((b.states[0], b.results[0], b.detail[0]), (STATE_REFUSED, E_MISSING, [hash, 0]));
        assert_eq!(records::module(0).used, 0, "the module was not placed");
        // the site stayed stock and the table holds no site
        assert_eq!(l.installer.count(), 0);
    }
}

#[test]
fn the_real_registry_accepts_zero_and_its_own_fingerprint_only() {
    let r = RealRegistry;
    assert!(r.abi_ok(0));
    assert!(r.abi_ok(dkrt_abi::service::SVC_ABI));
    assert!(!r.abi_ok(dkrt_abi::service::SVC_ABI ^ 1));
}

#[test]
fn capture_keeps_a_table_that_fits_and_ignores_a_short_file_a_bad_index_an_empty_table_and_one_over_the_limit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.reset_plan();
        let file = module_file();
        l.capture(0, &file);
        assert_eq!(l.table_len[0], 8);
        assert_eq!(l.tables[0][..8], file[file.len() - 8..]);
        // nothing is kept for a file shorter than the header, an index past the selection, an empty table or one that lies outside the file
        l.reset_plan();
        l.capture(1, &file[..dkneeds::HEADER as usize - 4]);
        l.capture(SEL_MAX, &file);
        l.capture(usize::MAX, &file);
        let mut empty = file.clone();
        put(&mut empty, dkneeds::H_REG_SIZE, 0);
        l.capture(2, &empty);
        let mut outside = file.clone();
        let end = outside.len() as u32;
        put(&mut outside, dkneeds::H_REG_OFF, end - 4);
        l.capture(3, &outside);
        assert_eq!(l.table_len, [0; SEL_MAX]);
        // a table of the planner's limit is kept, one byte more is not (the loader decides about such a file)
        let mut long = file.clone();
        long.resize(long.len() + TABLE_MAX, 0);
        put(&mut long, dkneeds::H_REG_SIZE, TABLE_MAX as u32);
        l.capture(4, &long);
        assert_eq!(l.table_len[4], TABLE_MAX);
        put(&mut long, dkneeds::H_REG_SIZE, TABLE_MAX as u32 + 1);
        l.capture(5, &long);
        assert_eq!(l.table_len[5], 0);
    }
}

#[test]
fn a_provider_the_admission_refused_is_reported_and_never_staged_and_its_dependents_are_refused() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the provider declares the machine kind, which a service of the table owns already: the rules of the core refuse it before anything of it is read again
        let kinds = [dkneeds::keys::KindDecl { kind: dkneeds::KIND_MACHINE, len: 0x6c, entry: 0 }];
        let mut l = staged_boot(service_file(&dkneeds::keys::Table { kinds: &kinds, ..svc_table() }, 0), consumer_file(&user_table()));
        l.find_providers(1);
        assert_eq!(l.found.n, 1);
        l.m.log.clear();
        let allocs = l.m.allocs;
        let _rt = boot_selection(&mut l, &[PATH]);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Stage)), "the refused file is not read, and nor is the module that needs it");
        assert_eq!((l.m.allocs, records::module(MAX_MODULES).used), (allocs, 0));
        assert!(l.phase == Phase::Closed);
        let hash = dkneeds::keys::name_hash(SVC);
        let b = crate::boot::info();
        assert_eq!((b.states[0], b.results[0], b.detail[0]), (STATE_REFUSED, E_MISSING, [hash, 0]));
        assert!(!l.accepted[0], "a module whose service was refused takes no number");
    }
}

#[test]
fn the_search_list_keeps_the_first_24_distinct_hard_needs_the_runtime_does_not_meet() {
    use dkneeds::keys::{name_hash, Need};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let need = |n: &str, hard: bool| Need { service: name_hash(n), min_version: 0, hard };
    let svc = |i: usize| std::format!("svc{i}");
    let mut tables = vec![needs_table(&[need("runtime", true), need("soft", false)]), needs_table(&[need(&svc(0), true), need(&svc(1), true)])];
    for i in 2..30 {
        tables.push(needs_table(&[need(&svc(i), true), need(&svc(0), true)]));
    }
    let mods: Vec<Option<Keys>> = tables.iter().map(|b| Some(Keys::parse(b).unwrap())).collect();
    let mut out = [0u32; WANT_MAX];
    let n = unmet_needs(&mods, &mut out);
    assert_eq!(n, WANT_MAX);
    let first: Vec<u32> = (0..WANT_MAX).map(|i| name_hash(&svc(i))).collect();
    assert_eq!(out.to_vec(), first, "in the order of the selection, each once; the runtime's service and the soft need are not in it");
    // a module with no table adds nothing
    let none: [Option<Keys>; 2] = [None, None];
    assert_eq!(unmet_needs(&none, &mut out), 0);
}

#[test]
fn the_services_are_handed_the_keys_of_admitted_modules_only() {
    use dkneeds::keys::{write_table, KeyGroup, Table};
    let keys = [0x100u32, 0x200];
    let t = Table { keys: &[KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &keys }], ..Default::default() };
    let mut bytes = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut bytes).unwrap();
    let mut tables = [[0u8; TABLE_MAX]; SEL_MAX];
    let mut lens = [0usize; SEL_MAX];
    for k in 0..2 {
        tables[k][..bytes.len()].copy_from_slice(&bytes);
        lens[k] = bytes.len();
    }
    // module 1 repeats the keys of module 0: admission refused it (E_PLAN_KEY), so its keys never reach the sticky assignment
    let accepted = {
        let mut a = [true; SEL_MAX];
        a[1] = false;
        a
    };
    let sel = settle_ctx(&accepted, &tables, &lens, 2);
    assert_eq!((sel.count, sel.nkeys[0], sel.nkeys[1]), (2, 2, 0));
    assert_eq!(sel.keys[0][..2], keys);
}

#[test]
fn keys_of_gives_the_keys_of_a_kind_for_an_accepted_module_only() {
    use dkneeds::keys::{write_table, KeyGroup, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (shapes, machines) = ([0x300u32, 0x400, 0x500], [0x100u32]);
    let t = Table {
        keys: &[
            KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &machines },
            KeyGroup { kind: dkneeds::KIND_LFO_SHAPE, keys: &shapes },
        ],
        ..Default::default()
    };
    let mut bytes = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut bytes).unwrap();
    unsafe {
        let mut l = loader(Init::Ok);
        l.reset_plan();
        for k in 0..2 {
            l.tables[k][..bytes.len()].copy_from_slice(&bytes);
            l.table_len[k] = bytes.len();
        }
        // before settle nothing is accepted
        let mut out = [0u32; 8];
        assert_eq!(l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
        l.accepted[0] = true;
        assert_eq!(l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut out), 3);
        assert_eq!(out[..3], shapes);
        // another kind, a kind the table does not list, a module the plan refused and an index past the selection
        assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), 1);
        assert_eq!(l.keys_of(0, dkneeds::KIND_EFFECT, &mut out), 0);
        assert_eq!(l.keys_of(1, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
        assert_eq!(l.keys_of(SEL_MAX as u32, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
    }
}
