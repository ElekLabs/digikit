use super::*;
use std::vec::Vec;

const SCHEMA: &[u8] = b"\x01\x04Tool\x02\x01k\x04Name\x04\x00\x00\x01q\x05Other\x40\x00\x01";
const FILE_SCHEMA: &[u8] = b"\x01\x04Tool\x01\x01k\x04Name\x2f\x01\x01\x04dkbs\x04None";

fn request(
    verb: u8,
    instance: u32,
    ticket: u32,
    field: u8,
    revision: u32,
    value: &[u8],
) -> Vec<u8> {
    let mut body = std::vec![verb, 0];
    body.extend_from_slice(&instance.to_be_bytes());
    if verb != DESCRIBE {
        body.extend_from_slice(&ticket.to_be_bytes());
    }
    if (GET..=RESET).contains(&verb) {
        body.push(field);
        body.extend_from_slice(&revision.to_be_bytes());
        body.extend_from_slice(value);
    }
    body
}

#[test]
fn descriptor_checks_its_full_shape_and_settings_namespace() {
    let schema = Descriptor::parse(b"test/m", SCHEMA).unwrap();
    assert_eq!(schema.bytes(), SCHEMA);
    assert_eq!(schema.field(1).unwrap().key(SCHEMA), b"q");
    assert!(schema.field(2).is_none());
    for size in 0..SCHEMA.len() {
        assert!(Descriptor::parse(b"test/m", &SCHEMA[..size]).is_none());
    }
    assert!(Descriptor::parse(b"test/m", &[SCHEMA, b"!"].concat()).is_none());
    assert!(Descriptor::parse(b"test/m", b"\x01\x01A\x01\x01/\x01V\x01\x00\x00").is_none());
    assert!(Descriptor::parse(
        b"test/m",
        b"\x01\x01A\x02\x01k\x01V\x01\x00\x00\x01k\x01X\x01\x00\x00"
    )
    .is_none());
    assert!(Descriptor::parse(b"test/m", b"\x01\x01A\x01\x01k\x01\xff\x01\x00\x00").is_none());

    let schema = Descriptor::parse(b"test/m", FILE_SCHEMA).unwrap();
    assert_eq!(schema.field(0).unwrap().extension(FILE_SCHEMA), b"dkbs");
    for (index, byte) in [
        (14, 48),
        (14, 5),
        (15, 2),
        (16, 2),
        (17, 0),
        (17, 17),
        (18, b'D'),
        (18, b'.'),
    ] {
        let mut invalid = FILE_SCHEMA.to_vec();
        invalid[index] = byte;
        assert!(
            Descriptor::parse(b"test/m", &invalid).is_none(),
            "byte {index} = {byte}"
        );
    }
}

#[test]
fn ticket_retries_keep_the_original_operation_and_completed_reply() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = Registry::EMPTY;
    reg.modules[0].descriptor = Descriptor::parse(b"test/m", SCHEMA).unwrap();
    let mut out = [0; DESCRIPTOR_MAX];
    let describe = request(DESCRIBE, 1, 0, 0, 0, b"");
    assert_eq!(reg.handle(&describe, &mut out), Err(E_NOT_FOUND));
    reg.publish(0);
    let instance = reg.modules[0].instance;
    let describe = request(DESCRIBE, instance, 0, 0, 0, b"");
    assert_eq!(reg.handle(&describe, &mut out), Ok(SCHEMA.len()));
    assert_eq!(&out[..SCHEMA.len()], SCHEMA);
    let put = request(PUT, instance, 5, 0, 1, b"ok");
    let poll = request(POLL, instance, 5, 0, 0, b"");
    assert_eq!(
        reg.handle(&request(PUT, instance, 8, 0, 1, b"long!"), &mut out),
        Err(E_SET_VALUE)
    );
    assert_eq!(
        reg.handle(&request(PUT, instance, 8, 2, 1, b"x"), &mut out),
        Err(E_SET_KEY)
    );
    assert_eq!(
        reg.handle(&request(PUT, instance, 8, 0, 1, b"\xe9"), &mut out),
        Err(E_SET_VALUE)
    );
    assert_eq!(reg.handle(&put, &mut out), Ok(0));
    assert_eq!(reg.handle(&put, &mut out), Ok(0));
    assert_eq!(
        reg.handle(&request(RESET, instance, 5, 0, 1, b""), &mut out),
        Err(E_MSG)
    );
    assert_eq!(
        reg.handle(&request(GET, instance, 6, 0, 0, b""), &mut out),
        Err(E_BUSY)
    );
    assert_eq!(reg.handle(&poll, &mut out), Ok(1));
    assert_eq!(out[0], 1);
    unsafe { module_cmds::set_draining(1, true) };
    let retry = reg.handle(&put, &mut out);
    let waiting = reg.handle(&poll, &mut out);
    unsafe { module_cmds::set_draining(1, false) };
    assert_eq!(
        retry,
        Ok(0),
        "a retry of an accepted write keeps the original ticket"
    );
    assert_eq!(waiting, Ok(1), "polling stays available during live unload");
    reg.modules[0].complete(Outcome {
        result: E_MODULE_UI_CONFLICT,
        revision: 27,
        ..Outcome::EMPTY
    });
    assert_eq!(reg.handle(&put, &mut out), Ok(0));
    assert_eq!(reg.handle(&poll, &mut out), Ok(10));
    assert_eq!(&out[..10], &[0, 255, 255, 255, 168, 0, 0, 0, 27, 0]);

    reg.modules[0] = Enrollment::EMPTY;
    reg.modules[0].descriptor = Descriptor::parse(b"test/m", SCHEMA).unwrap();
    reg.publish(0);
    assert_eq!(reg.handle(&poll, &mut out), Err(E_NOT_FOUND));
    assert_eq!(reg.handle(&put, &mut out), Err(E_NOT_FOUND));
}

unsafe fn perform(
    instance: u32,
    ticket: u32,
    verb: u8,
    revision: u32,
    value: &[u8],
) -> (i32, u32, Option<Vec<u8>>) {
    perform_at(instance, ticket, verb, 0, revision, value)
}

unsafe fn perform_at(
    instance: u32,
    ticket: u32,
    verb: u8,
    field: u8,
    revision: u32,
    value: &[u8],
) -> (i32, u32, Option<Vec<u8>>) {
    let mut out = [0; DESCRIPTOR_MAX];
    let submit = request(verb, instance, ticket, field, revision, value);
    assert_eq!(locked(|r| r.handle(&submit, &mut out)), Ok(0));
    tick();
    let poll = request(POLL, instance, ticket, 0, 0, b"");
    let len = locked(|r| r.handle(&poll, &mut out)).unwrap();
    assert_eq!(out[0], 0);
    (
        i32::from_be_bytes(out[1..5].try_into().unwrap()),
        u32::from_be_bytes(out[5..9].try_into().unwrap()),
        (out[9] != 0).then(|| out[10..len].to_vec()),
    )
}

#[test]
fn workbench_edits_use_module_settings_and_reject_stale_revisions() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    unsafe {
        use crate::{dlog, settings};
        settings::tests::setup(false);
        let ctx = entry::bind(0) as *mut u8;
        files::init_begin(0);
        let prev = dlog::enter_module(0, dlog::PHASE_INIT);
        let enroll = || dk_workbench_register(ctx, SCHEMA.as_ptr(), SCHEMA.len() as u32);
        assert_eq!(
            dk_workbench_register(core::ptr::null_mut(), SCHEMA.as_ptr(), SCHEMA.len() as u32),
            E_CONTEXT
        );
        assert_eq!(enroll(), OK);
        assert_eq!(enroll(), E_STATE);
        dlog::leave_module(prev);
        files::ready(0);
        assert_eq!(enroll(), E_CONTEXT);
        assert_eq!(instance(0), 0);
        publish(0);
        let first = instance(0);
        assert_ne!(first, 0);

        let (status, rev, value) = perform(first, 1, GET, 0, b"");
        assert_eq!((status, value), (OK, None));
        let (status, empty, value) = perform(first, 2, PUT, rev, b"");
        assert_eq!((status, value), (OK, Some(Vec::new())));
        assert_ne!(empty, rev);
        assert_eq!(
            perform(first, 3, PUT, empty, b""),
            (OK, empty, Some(Vec::new()))
        );

        let key = b"k\0".as_ptr();
        assert_eq!(entry::module_put(1, key, b"away".as_ptr(), 4), OK);
        assert_eq!(entry::module_put(1, key, b"".as_ptr(), 0), OK);
        let (status, returned, value) = perform(first, 4, RESET, empty, b"");
        assert_eq!((status, value), (E_MODULE_UI_CONFLICT, Some(Vec::new())));
        assert_ne!(
            returned, empty,
            "A-B-A must invalidate the original revision"
        );

        let (status, cleared, value) = perform(first, 5, RESET, returned, b"");
        assert_eq!((status, value), (OK, None));
        assert_ne!(cleared, returned);
        assert_eq!(perform(first, 6, RESET, cleared, b""), (OK, cleared, None));

        files::retire(0);
        entry::unbind(0);
        settings::tests::teardown();
        assert_eq!(instance(0), 0);
    }
}

#[test]
fn workbench_reads_the_overlay_but_edits_require_device_scope() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    unsafe {
        use crate::settings;
        settings::tests::setup(true);
        assert_eq!(
            entry::module_put(1, b"k\0".as_ptr(), b"old".as_ptr(), 3),
            OK
        );
        locked(|r| r.modules[0].descriptor = Descriptor::parse(b"test/m", SCHEMA).unwrap());
        publish(0);
        let token = instance(0);
        let (status, rev, value) = perform(token, 1, GET, 0, b"");
        assert_eq!((status, value), (OK, Some(b"old".to_vec())));
        assert_eq!(
            perform(token, 2, PUT, rev, b"new"),
            (E_MODULE_UI_SCOPE, rev, Some(b"old".to_vec()))
        );
        assert_eq!(
            perform(token, 3, RESET, rev, b""),
            (E_MODULE_UI_SCOPE, rev, Some(b"old".to_vec()))
        );
        assert_eq!(
            settings::tests::overlay_value(b"test/m/k"),
            Some(b"old".to_vec())
        );

        settings_overlay::withdraw(0);
        let (status, new_rev, value) = perform(token, 4, PUT, rev, b"new");
        assert_eq!(
            (status, value),
            (E_MODULE_UI_CONFLICT, Some(b"old".to_vec()))
        );
        assert_ne!(new_rev, rev);
        assert_eq!(perform(token, 5, PUT, new_rev, b"new").0, OK);
        withdraw(0);
        settings::tests::teardown();
    }
}

#[test]
fn workbench_file_edits_require_an_ascii_leaf_in_the_modules_own_data_directory() {
    use crate::store_tests::{upload, Fixture};
    let _card = Fixture::new();
    assert_eq!(upload("data/other/m/foreign.dkbs", b"file"), OK);
    assert_eq!(upload("data/test/m/intro.DKBS", b"file"), OK);
    unsafe {
        use crate::settings;
        settings::tests::setup(false);
        locked(|r| r.modules[0].descriptor = Descriptor::parse(b"test/m", FILE_SCHEMA).unwrap());
        publish(0);
        let token = instance(0);
        let (_, rev, _) = perform(token, 1, GET, 0, b"");
        for value in [
            &b""[..],
            b".dkbs",
            b"a.wav",
            b"a..b.dkbs",
            b"dir/a.dkbs",
            b"dir\\a.dkbs",
            b"a\0.dkbs",
            b"a\x7f.dkbs",
            "é.dkbs".as_bytes(),
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.dkbs",
        ] {
            let cmd = request(PUT, token, 2, 0, rev, value);
            assert_eq!(
                locked(|r| r.handle(&cmd, &mut [0; DESCRIPTOR_MAX])),
                Err(E_SET_VALUE)
            );
        }
        assert_eq!(
            perform(token, 3, PUT, rev, b"foreign.dkbs"),
            (E_NOT_FOUND, rev, None)
        );
        assert_eq!(
            perform(token, 4, PUT, rev, b"intro.dkbs"),
            (E_NOT_FOUND, rev, None)
        );
        let (status, changed, value) = perform(token, 5, PUT, rev, b"intro.DKBS");
        assert_eq!((status, value), (OK, Some(b"intro.DKBS".to_vec())));
        assert_ne!(changed, rev);
        withdraw(0);
        settings::tests::teardown();
    }
}

#[test]
fn typed_writes_are_checked_with_the_shared_vectors() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let descriptors = vectors::descriptors();
    let mut out = [0; DESCRIPTOR_MAX];
    for value in vectors::values() {
        let vector = descriptors
            .iter()
            .find(|d| d.name == value.descriptor)
            .unwrap();
        let mut reg = Registry::EMPTY;
        reg.modules[0].descriptor =
            Descriptor::parse(vector.module.as_bytes(), &vector.bytes).unwrap();
        reg.publish(0);
        let instance = reg.modules[0].instance;
        let index = reg.modules[0]
            .descriptor
            .key_index(value.setting.as_bytes())
            .unwrap();
        let put = request(PUT, instance, 1, index as u8, 1, &value.bytes);
        let expected = if value.valid { Ok(0) } else { Err(E_SET_VALUE) };
        assert_eq!(reg.handle(&put, &mut out), expected, "{}", value.rule);
    }
}

/// Enroll `descriptor` for module 0 and publish it; returns the instance. The caller retires and tears down.
unsafe fn enroll(descriptor: &[u8]) -> u32 {
    let ctx = entry::bind(0) as *mut u8;
    files::init_begin(0);
    let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_INIT);
    assert_eq!(
        dk_workbench_register(ctx, descriptor.as_ptr(), descriptor.len() as u32),
        OK
    );
    crate::dlog::leave_module(prev);
    files::ready(0);
    publish(0);
    instance(0)
}

unsafe fn retire() {
    withdraw(0);
    files::retire(0);
    entry::unbind(0);
    crate::settings::tests::teardown();
}

#[test]
fn stored_values_are_checked_on_read_with_the_shared_vectors() {
    // The store takes any bytes: a value that the write path would refuse (an older file, another writer) still reads as invalid.
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let values = vectors::values();
    for vector in vectors::descriptors() {
        unsafe {
            crate::settings::tests::setup(false);
            let instance = enroll(&vector.bytes);
            let descriptor = Descriptor::parse(vector.module.as_bytes(), &vector.bytes).unwrap();
            let mut ticket = 0;
            for value in values.iter().filter(|v| v.descriptor == vector.name) {
                let index = descriptor.key_index(value.setting.as_bytes()).unwrap();
                let key = std::format!("{}\0", value.setting);
                assert_eq!(
                    entry::module_put(
                        1,
                        key.as_ptr(),
                        value.bytes.as_ptr(),
                        value.bytes.len() as u32
                    ),
                    OK
                );
                ticket += 1;
                let (status, _, read) = perform_at(instance, ticket, GET, index as u8, 0, b"");
                let expected = if value.valid {
                    (OK, Some(value.bytes.clone()))
                } else {
                    (E_SET_VALUE, None)
                };
                assert_eq!((status, read), expected, "{}", value.rule);
            }
            retire();
        }
    }
}

#[test]
fn a_stored_file_name_is_checked_on_read_against_the_declared_extension() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    unsafe {
        crate::settings::tests::setup(false);
        let instance = enroll(FILE_SCHEMA);
        for (ticket, (name, valid)) in [
            (&b"intro.dkbs"[..], true),
            (b"INTRO.DKBS", true),
            (b"intro.txt", false),
            (b"a/b.dkbs", false),
            (b"..dkbs", false),
            (b"\xff.dkbs", false),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                entry::module_put(1, b"k\0".as_ptr(), name.as_ptr(), name.len() as u32),
                OK
            );
            let (status, _, read) = perform(instance, ticket as u32 + 1, GET, 0, b"");
            let expected = if valid {
                (OK, Some(name.to_vec()))
            } else {
                (E_SET_VALUE, None)
            };
            assert_eq!((status, read), expected, "{name:?}");
        }
        retire();
    }
}

#[test]
fn typed_kinds_do_not_grow_the_registry() {
    // Static RAM: bounds are read from the stored descriptor bytes, a field keeps offsets only. Unchanged by the typed kinds.
    assert_eq!(core::mem::size_of::<Descriptor>(), 1092);
    assert_eq!(core::mem::size_of::<Registry>(), 19088);
}
