//! The payloads of a package over the real packed files of the fixtures `local-payload` (a service that adopts the payloads part) and `local-payload-user` (a module that carries a slow
//! and a fast body for it), through the manager over the runtime's seams: each payload goes to its owner by its kind, a pending install is stepped to Active, a refusal undoes the walk,
//! a pending retire keeps the take-down waiting, and the check of the owner runs before anything is placed and in a reload's check. The edge cases of the checks are unit tests
//! (`payloads.rs` of the loader tests); here is what needs the packed files. The host runs no ColdFire code: `init` of the service replays its declarations and the row is the fixture's.
use super::*;
use crate::package::lifecycle::{Rt, RtManifest, Session};
use dkneeds::keys::{name_hash, Keys};
use dkrt_abi::package::PackageId;
use dkrt_lifecycle::State;

fn id(n: usize) -> PackageId {
    PackageId(n as u8)
}

/// What the service saw (`OP_SEEN` through the query part of its row in the table): kind 1 installed, kind 2 installed, retired, install calls, body length of the last first call and its
/// consumer.
fn seen() -> [u32; 6] {
    let hash = name_hash("local-payload");
    let h = crate::package::provider_rows::rows().iter().find(|r| r.name() == hash).expect("the row of local-payload is in the table");
    let mut reply = [0u8; dkrt_fixture_local_payload::SEEN_LEN];
    assert_eq!(unsafe { h.query(dkrt_fixture_local_payload::OP_SEEN, &[], &mut reply) }, reply.len() as i32);
    let mut out = [0u32; 6];
    for (i, w) in out.iter_mut().enumerate() {
        *w = u32::from_be_bytes([reply[i * 4], reply[i * 4 + 1], reply[i * 4 + 2], reply[i * 4 + 3]]);
    }
    out
}

/// The offset of the body of payload `i` of the packed file (its item in the header table).
fn body_at(file: &[u8], i: usize) -> usize {
    let t = table_of(file);
    Keys::parse(&t).unwrap().payload(i).expect("the file has the payload").offset as usize
}

/// `planned`, with `edit` applied to the packed consumer before the loader reads it (the table is captured from the edited file): the variants of a fixture that the pack does not
/// make. `seal` puts the size and the hash right again.
unsafe fn planned_edited(dev: Dev, image: &Image, edit: &dyn Fn(&mut Vec<u8>)) -> (Boot, Plan) {
    let mut user = packed(dev, LOCAL_PAYLOAD_USER.leaf, "dkm", LOCAL_PAYLOAD_USER.nx);
    edit(&mut user);
    seal(&mut user);
    let mut files = card_of(dev, &[&LOCAL_PAYLOAD_USER], &[LOCAL_PAYLOAD, LOCAL_PING]);
    let at = files.iter().position(|(n, _)| n == "modules/local-payload-user.dkm").unwrap();
    files[at].1 = user.clone();
    let p = image.platform(dev, &[]);
    for (addr, bytes) in stock_sites(dev).values() {
        p.put_code(*addr, bytes);
    }
    let mut l = Loader::new(p, Card::new(files, dev.profile.id), DeviceRegistry { runtime: package_object() });
    l.reset_plan();
    l.capture(0, &user);
    l.find_providers(1);
    let np = l.found.n;
    (l, Plan { n: 1, np })
}

const USER: usize = 0;
fn service(l: &Boot) -> PackageId {
    let j = (0..l.found.n).find(|j| l.found.path(*j) == b"services/local-payload.dks").expect("the search found the service");
    id(MAX_MODULES + j)
}

#[test]
fn each_payload_goes_to_its_owner_by_kind_and_a_pending_install_is_stepped_to_active() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(dev, &image, &[], &[&LOCAL_PAYLOAD_USER], &[LOCAL_PAYLOAD, LOCAL_PING]);
            let mut run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], true);
            assert!(run.report.refused().is_empty(), "{}: {:?}", dev.name, run.report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            let service = service(&l);
            assert_eq!(run.report.brought_up().as_slice(), [service, id(USER)], "{}", dev.name);
            // the first attach call made the first install call of both: the fast kind is done, the slow one answered Pending (the service gets each body then, with its consumer)
            assert_eq!(run.report.attaching().as_slice(), [id(USER)], "{}: the consumer is still attaching", dev.name);
            assert_eq!(run.pm.state_of(id(USER)), Some(State::Attaching), "{}", dev.name);
            assert!(run.is_up(id(USER)), "{}: live while it attaches", dev.name);
            // [slow, fast, retired, calls, length of the last body, consumer]: the fast body is one byte, the consumer is record 0 + 1
            assert_eq!(seen(), [0, 1, 0, 2, 1, 1], "{}", dev.name);

            // each pass asks the slow payload again with no body, until it is done: it answers Pending three times (the first body byte; the first call was the first), so two
            // passes keep it attaching and the third makes it Active
            for pass in 0..2 {
                let r = run.pm.step();
                assert_eq!(r.attaching().as_slice(), [id(USER)], "{}: pass {pass}", dev.name);
                assert_eq!(seen()[3], 3 + pass, "{}: one more install call a pass", dev.name);
            }
            let r = run.pm.step();
            assert_eq!(r.brought_up().as_slice(), [id(USER)], "{}", dev.name);
            assert!(r.attaching().is_empty());
            assert_eq!(run.pm.state_of(id(USER)), Some(State::Active), "{}", dev.name);
            // each payload was installed once, the later calls carried no body (the length is the one of the first calls)
            assert_eq!(seen(), [1, 1, 0, 5, 1, 1], "{}", dev.name);
            assert_eq!(run.pm.step(), Report::default(), "{}: nothing waits any more", dev.name);

            // the unload has the owner retire once for the consumer (not once for each payload), then the records go
            let r = run.pm.unload(id(USER)).unwrap();
            assert_eq!(r.taken_down().as_slice(), [id(USER)], "{}: the service is pinned", dev.name);
            assert_eq!(seen()[2], 1, "{}: retired once for the consumer", dev.name);
            let _ = service;
        }
        clean_state();
    }
}

#[test]
fn a_retire_that_is_pending_keeps_the_take_down_waiting_and_a_retry_finishes_it() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the slow body: no Pending answers of the install, two of the retire
            let (mut l, plan) = planned_edited(dev, &image, &|f| {
                let at = body_at(f, 0);
                f[at] = 0;
                f[at + 1] = 2;
            });
            let mut run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], true);
            assert!(run.report.refused().is_empty() && run.report.attaching().is_empty(), "{}", dev.name);
            assert_eq!(run.pm.state_of(id(USER)), Some(State::Active), "{}", dev.name);
            let service = service(&l);
            assert!(matches!(run.pm.unload(id(USER)), Err(LifecycleError::Busy { .. })), "{}", dev.name);
            assert_eq!(run.pm.state_of(id(USER)), Some(State::Detaching), "{}", dev.name);
            assert_eq!(run.pm.restore_pending(), Some(id(USER)));
            assert_eq!(seen()[2], 0, "{}: nothing retired yet", dev.name);
            assert!(matches!(run.pm.retry_pending(), Err(LifecycleError::Busy { .. })), "{}", dev.name);
            assert_eq!(seen()[2], 0, "{}: the second retire is still pending", dev.name);
            let r = run.pm.retry_pending().unwrap();
            assert_eq!(r.taken_down().as_slice(), [id(USER)], "{}", dev.name);
            assert_eq!(seen()[2], 1, "{}: retired once, when it was done", dev.name);
            assert!(run.pm.restore_pending().is_none());
            let _ = service;
        }
        clean_state();
    }
}

#[test]
fn a_payload_the_owner_refuses_undoes_the_walk_and_refuses_the_consumer() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the install of the slow kind refuses (0xff): the first attach call undoes the walk, so the manager sees the refusal and nothing is attached
            let (mut l, plan) = planned_edited(dev, &image, &|f| {
                let at = body_at(f, 0);
                f[at] = dkrt_fixture_local_payload::REFUSE_INSTALL;
            });
            let run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], true);
            assert_eq!(run.code(USER), dkrt_sdk::E_INIT, "{}", dev.name);
            assert!(!run.is_up(id(USER)), "{}", dev.name);
            assert!(run.report.attaching().is_empty(), "{}", dev.name);
            // the owner gave back what the consumer had (a retire for it) and installed nothing
            let s = seen();
            assert_eq!((s[0], s[1], s[2]), (0, 0, 1), "{}: {s:?}", dev.name);
            // the services stay (pinned by the test) and the consumer's memory is back
            assert!(run.is_up(service(&l)), "{}", dev.name);
            assert_eq!(st().modules[USER].used, 0, "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn the_owner_checks_the_body_before_anything_is_placed() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the check of the slow kind refuses this body (0xfe): the consumer is refused at its place, the owner has seen no install
            let (mut l, plan) = planned_edited(dev, &image, &|f| {
                let at = body_at(f, 0);
                f[at] = dkrt_fixture_local_payload::REFUSE_CHECK;
            });
            let run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], true);
            assert_eq!(run.code(USER), dkrt_sdk::E_FORMAT, "{}", dev.name);
            assert_eq!(seen(), [0, 0, 0, 0, 0, 0], "{}: nothing was installed", dev.name);
            assert_eq!(st().modules[USER].used, 0, "{}: nothing was placed", dev.name);
        }
        clean_state();
    }
}

#[test]
fn an_owner_without_the_payloads_part_refuses_a_consumer_that_names_it() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            // the same file, naming `local-ping` (which adopts no payloads part) as the owner of both bodies and as its hard need
            let from = name_hash("local-payload").to_be_bytes();
            let to = name_hash("local-ping").to_be_bytes();
            let (mut l, plan) = planned_edited(dev, &image, &|f| {
                let t = {
                    let (off, len) = (be(f, dkneeds::keys::H_KEYS_OFF) as usize, be(f, dkneeds::keys::H_KEYS_SIZE) as usize);
                    off..off + len
                };
                let mut at = t.start;
                while at + 4 <= t.end {
                    if f[at..at + 4] == from {
                        f[at..at + 4].copy_from_slice(&to);
                    }
                    at += 4;
                }
            });
            // the search finds `local-ping` for the need, as it is the file of that name
            let run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], true);
            assert_eq!(run.code(USER), dkrt_sdk::E_MISSING, "{}: {:?}", dev.name, run.report.refused().iter().map(|r| r.reason).collect::<Vec<_>>());
            assert_eq!(st().modules[USER].used, 0, "{}", dev.name);
        }
        clean_state();
    }
}

#[test]
fn a_reload_check_asks_the_owner_about_the_new_file() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(dev, &image, &[], &[&LOCAL_PAYLOAD_USER], &[LOCAL_PAYLOAD, LOCAL_PING]);
            let mut run = boot_run(&mut l, &plan, &[&LOCAL_PAYLOAD_USER], false);
            for _ in 0..4 {
                run.pm.step();
            }
            assert_eq!(run.pm.state_of(id(USER)), Some(State::Active), "{}", dev.name);
            // the new file of the module is the same but for a body its owner refuses
            let path = b"modules/local-payload-user.dkm".to_vec();
            let mut bad = packed(dev, LOCAL_PAYLOAD_USER.leaf, "dkm", LOCAL_PAYLOAD_USER.nx);
            let at = body_at(&bad, 0);
            bad[at] = dkrt_fixture_local_payload::REFUSE_CHECK;
            seal(&mut bad);
            let good = l.m.files.iter().find(|(n, _)| *n == path).unwrap().1.clone();
            l.m.files.iter_mut().find(|(n, _)| *n == path).unwrap().1 = bad;
            let caps = live_cap(&l);
            let p = b"modules/local-payload-user.dkm\0";
            assert_eq!(l.reload_check(USER, p.as_ptr(), &caps), dkrt_sdk::E_FORMAT, "{}: the owner refuses the body before anything is taken down", dev.name);
            assert!(run.is_up(id(USER)), "{}: the old module is untouched", dev.name);
            l.m.files.iter_mut().find(|(n, _)| *n == path).unwrap().1 = good;
            assert_eq!(l.reload_check(USER, p.as_ptr(), &caps), OK, "{}: the unchanged file passes", dev.name);
        }
        clean_state();
    }
}
