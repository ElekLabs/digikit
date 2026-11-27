//! The state handoff of a service reload over the real packed `local-ping` fixtures (docs/plans, hot reload slice 2, S4): the fixture counts the consumers that ever attached
//! (`count`, its `State` part), and the closure reload carries the count to the next copy when the version range allows, drops it when it does not, logs which of the two
//! happened (`STATE_CARRIED`), and a refusal for lack of room leaves everything up. What the host cannot show stays out: the bytes of the count are the fixture's own.
use super::assembly_tests::*;
use super::manager_assembly_tests::{id, local_ping_v2};
use super::*;
use crate::dlog::{self, Ev};
use crate::package::lifecycle::{Rt, RtManifest};
use dkrt_fixture_local_ping::{OP_COUNT, STATE_VERSION};
use dkrt_sdk::logk;

static mut SEEN: Vec<Ev> = Vec::new();

unsafe extern "C" fn keep(e: *const Ev) {
    if (*e).kind == logk::STATE_CARRIED {
        (*core::ptr::addr_of_mut!(SEEN)).push(*e);
    }
}

/// Take the log events from here on: a sink that is set first gets the events the log kept, so what came before is dropped.
unsafe fn watch() {
    dlog::dk_debug_log_sink_set(keep as *const () as usize as *mut u8, core::ptr::null_mut());
    (*core::ptr::addr_of_mut!(SEEN)).clear();
}

/// The count of the running copy, asked through the query part of its row.
fn count() -> u32 {
    let hash = dkneeds::keys::name_hash("local-ping");
    let h = crate::package::provider_rows::rows().iter().find(|r| r.name() == hash).expect("the row of local-ping is in the table");
    let mut reply = [0u8; 4];
    assert_eq!(unsafe { h.query(OP_COUNT, &[], &mut reply) }, 4);
    u32::from_be_bytes(reply)
}

struct Case<'a> {
    l: Boot,
    rt: Rt<crate::fake_platform::FakePlatform, Card, DeviceRegistry>,
    path: Vec<u8>,
    dev: &'a str,
}

/// Boot the consumer and the service; the count is 1 (the consumer attached once).
unsafe fn booted(dev: Dev, image: &Image, file: Vec<u8>) -> Box<Case<'_>> {
    let (l, _plan) = planned(dev, image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
    let mut case = Box::new(Case { l, rt: core::mem::zeroed(), path: Vec::new(), dev: dev.name });
    core::ptr::write(&mut case.rt, Rt::new(&mut case.l as *mut _));
    let rec = boot_record(&[LOCAL_PING_USER.leaf]);
    case.rt.boot_with(&rec, &capacity(&case.l.p));
    assert!(case.rt.manager().unwrap().live().contains(id(MAX_MODULES)), "{}", dev.name);
    assert_eq!(count(), 1, "{}: the consumer attached once", dev.name);
    case.path = std::format!("services/{}.dks\0", LOCAL_PING.leaf).into_bytes();
    let name = case.path[..case.path.len() - 1].to_vec();
    case.l.m.files.iter_mut().find(|(n, _)| n == &name).unwrap().1 = file;
    case
}

#[test]
fn a_closure_reload_keeps_the_count_and_logs_it_as_carried() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let mut c = booted(dev, &image, local_ping_v2(dev));
            watch();
            assert_eq!(c.rt.reload_closure_with(MAX_MODULES, c.path.as_ptr(), &live_cap(&c.l)), dkrt_sdk::OK, "{}", c.dev);
            dlog::test_reset_sinks();
            assert!(!c.rt.reload_torn(), "{}", c.dev);
            // the new copy took 1 and the consumer attached to it again: the count a consumer reads after the reload is 2, not 1
            assert_eq!(count(), 2, "{}", c.dev);
            let seen: Vec<_> = (*core::ptr::addr_of!(SEEN)).iter().map(|e| e.a).collect();
            let hash = dkneeds::keys::name_hash("local-ping");
            assert_eq!(seen.len(), 1, "{}: one event for the one service with the part: {seen:?}", c.dev);
            assert_eq!(seen[0][1..], [hash, 0, STATE_VERSION as u32, 4], "{}: carried: code 0, the layout and the bytes", c.dev);
        }
        clean_state();
    }
}

#[test]
fn a_new_copy_whose_range_excludes_the_old_layout_starts_from_import_none_and_logs_it_as_dropped() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let mut c = booted(dev, &image, local_ping_v2(dev));
            // the new copy's row: its state part takes no layout the old copy has
            c.l.m.state_beyond = true;
            watch();
            assert_eq!(c.rt.reload_closure_with(MAX_MODULES, c.path.as_ptr(), &live_cap(&c.l)), dkrt_sdk::OK, "{}", c.dev);
            dlog::test_reset_sinks();
            // import(None) gave 0 and the consumer attached once
            assert_eq!(count(), 1, "{}", c.dev);
            let seen: Vec<_> = (*core::ptr::addr_of!(SEEN)).iter().map(|e| e.a).collect();
            let hash = dkneeds::keys::name_hash("local-ping");
            assert_eq!(seen.len(), 1, "{}: {seen:?}", c.dev);
            assert_eq!(seen[0][1..], [hash, 2, STATE_VERSION as u32, 4], "{}: dropped for the version (code 2), with the layout and bytes it had", c.dev);
        }
        clean_state();
    }
}

#[test]
fn a_carry_that_finds_no_room_refuses_the_reload_and_changes_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let mut c = booted(dev, &image, local_ping_v2(dev));
            let hash = dkneeds::keys::name_hash("local_ping_value");
            let v1 = c.l.exports.owned(hash).unwrap().0;
            // the next block the loader asks for is the state block of the old copy: the file's check takes none
            c.l.m.fail_alloc = Some(c.l.m.allocs + 1);
            watch();
            assert_eq!(c.rt.reload_closure_with(MAX_MODULES, c.path.as_ptr(), &live_cap(&c.l)), dkrt_sdk::E_NOMEM, "{}", c.dev);
            dlog::test_reset_sinks();
            assert!(!c.rt.reload_torn(), "{}", c.dev);
            assert!(c.rt.manager().unwrap().live().contains(id(0)) && c.rt.manager().unwrap().live().contains(id(MAX_MODULES)), "{}: both stay up", c.dev);
            assert_eq!(c.l.exports.owned(hash).unwrap().0, v1, "{}: the old copy still serves its export", c.dev);
            assert_eq!(count(), 1, "{}: the old copy's count is untouched", c.dev);
            assert!((*core::ptr::addr_of!(SEEN)).is_empty(), "{}: no handoff was logged: {:?}", c.dev, (*core::ptr::addr_of!(SEEN)).iter().map(|e| e.a).collect::<Vec<_>>());
            // the same reload goes through once memory is there again
            c.l.m.fail_alloc = None;
            assert_eq!(c.rt.reload_closure_with(MAX_MODULES, c.path.as_ptr(), &live_cap(&c.l)), dkrt_sdk::OK, "{}", c.dev);
            assert_eq!(count(), 2, "{}", c.dev);
        }
        clean_state();
    }
}
