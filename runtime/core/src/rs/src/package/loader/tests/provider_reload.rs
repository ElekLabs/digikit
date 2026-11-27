//! The checks of a service reload (`provider_check`, `provider_prepare`) and the teardown entries of a service (`reload_quiesce`, `reload_abort`).
use super::*;
use dkneeds::keys::{name_hash, Pair, Table};

const SVC_REC: usize = MAX_MODULES;
/// The store path of the service, NUL terminated.
const SVC_PATH0: &[u8] = b"services/svc-test.dks\0";

/// The table of the test service with the exports `exports` (name, image offset) and the PROVIDES version `version`.
fn svc_with(exports: &[(&str, u32)], version: u32) -> Vec<u8> {
    let mut e: Vec<Pair> = exports.iter().map(|(n, o)| Pair { a: name_hash(n), b: *o }).collect();
    e.sort_by_key(|p| p.a);
    let provides = [Pair { a: name_hash(SVC), b: version }];
    let hooks = [Pair { a: name_hash("hook.a"), b: 4 }];
    let t = Table { provides: &provides, exports: &e, hooks: &hooks, ..Default::default() };
    service_file(&t, 0)
}

/// A boot of the consumer `a` (needs `svc-test` at 0.8.2, imports its export `dk_svctest_ping`) over the service file `svc`: both are up, the service is provider record 0.
unsafe fn up() -> (Fake, Box<crate::package::lifecycle::Rt<FakePlatform, FakeImages, FakeRegistry>>) {
    let mut l = staged_boot(svc_with(&[("dk_svctest_ping", 8)], 0x000802), consumer_file(&user_table()));
    l.find_providers(1);
    let rt = boot_selection(&mut l, &[PATH]);
    assert_eq!((records::module(SVC_REC).used, records::module(0).used), (1, 1), "the boot brought the service and its consumer up");
    (l, rt)
}

fn put_svc(l: &mut Fake, file: Vec<u8>) {
    l.m.files.insert(SVC_PATH0[..SVC_PATH0.len() - 1].to_vec(), file);
}

#[test]
fn a_service_check_accepts_a_file_that_keeps_its_exports_and_changes_nothing() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, _rt) = up();
        let (base, sites, exports, table) = (records::module(SVC_REC).base, l.installer.count(), l.exports.count(), l.found.table(0).to_vec());
        let site = site_bytes(&l, 6);
        // the old exports and one more, and a newer minor
        put_svc(&mut l, svc_with(&[("dk_svctest_ping", 8), ("dk_svctest_pong", 12)], 0x000803));
        assert_eq!(l.provider_check(SVC_REC, SVC_PATH0.as_ptr()), OK, "the sites of the old record count as stock");
        assert_eq!((records::module(SVC_REC).base, l.installer.count(), l.exports.count(), l.found.table(0).to_vec(), site_bytes(&l, 6)), (base, sites, exports, table, site));
        assert_eq!(records::module(SVC_REC).used, 1);
    }
}

#[test]
fn a_service_check_refuses_a_dropped_export_another_name_a_new_major_a_damaged_file_and_a_dsp_part() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, _rt) = up();
        let mut check = |l: &mut Fake, f: Vec<u8>| {
            put_svc(l, f);
            l.provider_check(SVC_REC, SVC_PATH0.as_ptr())
        };
        assert_eq!(check(&mut l, svc_with(&[("dk_svctest_other", 8)], 0x000802)), E_RELOAD_KEYS, "a consumer imports the old export by name");
        let mut other = svc_with(&[("dk_svctest_ping", 8)], 0x000802);
        other[dkneeds::H_NAME as usize] = b'z';
        seal(&mut other);
        assert_eq!(check(&mut l, other), E_FORMAT);
        assert_eq!(check(&mut l, svc_with(&[("dk_svctest_ping", 8)], 0x000801)), E_CONTRACT, "the consumer needs 0.8.2 and the file offers less");
        let mut bad = svc_with(&[("dk_svctest_ping", 8)], 0x000802);
        bad[0] ^= 1;
        assert_eq!(check(&mut l, bad), E_FORMAT);
        let mut dsp = svc_with(&[("dk_svctest_ping", 8)], 0x000802);
        put(&mut dsp, dkneeds::H_DSP_SIZE, 64);
        seal(&mut dsp);
        assert_eq!(check(&mut l, dsp), E_BOOT_ONLY, "no DSP step in the closure reload");
        l.m.files.remove(&SVC_PATH0[..SVC_PATH0.len() - 1].to_vec());
        assert_eq!(l.provider_check(SVC_REC, SVC_PATH0.as_ptr()), E_NOT_FOUND, "the new file is not in the store");
        // a record that is no loaded service
        assert_eq!(l.provider_check(0, SVC_PATH0.as_ptr()), E_NOT_FOUND);
        assert_eq!(l.provider_check(SVC_REC + 1, SVC_PATH0.as_ptr()), E_NOT_FOUND);
        assert_eq!((records::module(SVC_REC).used, records::module(0).used), (1, 1));
    }
}

#[test]
fn a_service_check_takes_a_file_that_imports_the_settings_overlay_registration() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, _rt) = up();
        let plain = svc_with(&[("dk_svctest_ping", 8)], 0x000802);
        let f = service_file_of(module_file_with(&[], &[b"dkc_settings_overlay_set"]), &Table { provides: &[Pair { a: name_hash(SVC), b: 0x000802 }], exports: &[Pair { a: name_hash("dk_svctest_ping"), b: 8 }], ..Default::default() }, 0);
        assert_eq!(l.provider_rules(0, SVC_REC, &plain).map(|_| ()), Ok(()));
        assert_eq!(l.provider_rules(0, SVC_REC, &f).map(|_| ()), Ok(()), "a reload carries its per-project state (State part); only a first bring-up is refused (live.rs)");
    }
}

#[test]
fn a_service_prepare_keeps_the_new_table_for_the_graph_and_a_check_does_not() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, _rt) = up();
        let old = l.found.table(0).to_vec();
        put_svc(&mut l, svc_with(&[("dk_svctest_ping", 8), ("dk_svctest_pong", 12)], 0x000802));
        assert_eq!(l.provider_check(SVC_REC, SVC_PATH0.as_ptr()), OK);
        assert_eq!(l.found.table(0), &old[..], "the check keeps the old table");
        assert_eq!(l.provider_prepare(SVC_REC, SVC_PATH0.as_ptr()), OK);
        let kept = Keys::parse(l.found.table(0)).unwrap();
        assert!(kept.words(dkneeds::keys::tag::EXPORTS, 1).is_some(), "the table of the new file");
        // a file that changed since the check is held to the same rules
        put_svc(&mut l, svc_with(&[("dk_svctest_pong", 12)], 0x000802));
        let kept_len = l.found.table_len[0];
        assert_eq!(l.provider_prepare(SVC_REC, SVC_PATH0.as_ptr()), E_RELOAD_KEYS);
        assert_eq!(l.found.table_len[0], kept_len, "a refused file changes no table");
    }
}

#[test]
fn a_service_quiesce_stops_its_tick_and_an_abort_gives_it_back() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, _rt) = up();
        crate::package::entry::publish_tick(SVC_REC, 0x1234);
        assert_eq!(l.reload_quiesce(SVC_REC), E_BUSY, "the entries are closed, two blocks must pass");
        assert_eq!(crate::package::entry::tick_of(SVC_REC), 0, "no new tick call");
        l.p.tick(2);
        assert_eq!(l.reload_quiesce(SVC_REC), OK);
        assert!(l.reload_abort(SVC_REC));
        assert_eq!(crate::package::entry::tick_of(SVC_REC), 0x1234, "the old service gets its tick back");
        assert_eq!(l.torn[SVC_REC], 0);
        assert_eq!(l.reload_quiesce(SVC_REC + 1), E_NOT_FOUND, "an unused provider record");
        crate::package::entry::publish_tick(SVC_REC, 0);
    }
}
