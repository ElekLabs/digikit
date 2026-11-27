use super::*;
use crate::transport::{DKRT_UPLOAD_LAST, RBODY};
use core::ptr::{addr_of, null_mut, read_volatile};
use core::slice;
use dkrt_sdk::proto::{CMD_MODULES, CMD_MODULE_UI, E_SET_VALUE};

// The MODULE_UI verbs (the registry keeps them private).
const GET: u8 = 1;
const PUT: u8 = 2;

const PATH: &[u8] = b"modules/settings-a.dkm\0";
// V2: the text setting boot, the integer gain (0 to 100, 2 decimals, "dB", default 50), the boolean enabled and the choice mode (Off, Soft, Hard; default Soft).
const SCHEMA: &[u8] = b"\x02\x0dBoot settings\x04\x04boot\x0aBoot value\x10\x00\x00\x04gain\x04Gain\x04\x02\x00\x00\x00\x00\x00\x00\x00\x00d\x00\x00\x00\x01\x02\x00\x00\x002\x02dB\x07enabled\x07Enabled\x01\x03\x00\x01\x04mode\x04Mode\x01\x04\x00\x03\x01\x03Off\x04Soft\x04Hard";
const GAIN: u8 = 1;
const MODE: u8 = 3;

fn descriptor_offset(file: &[u8]) -> usize {
    let image_off = be(file, dkneeds::H_IMAGE_OFF) as usize;
    let image_size = be(file, dkneeds::H_IMAGE_SIZE) as usize;
    let image = &file[image_off..image_off + image_size];

    // The pack has no symbol for the generated static array. Locate the
    // fixture's expected descriptor in the packed image.
    let mut offsets = image
        .windows(SCHEMA.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == SCHEMA).then_some(at));
    let offset = offsets
        .next()
        .expect("packed settings-a Workbench descriptor");
    assert!(offsets.next().is_none(), "ambiguous packed descriptor");
    image_off + offset
}

/// The query table the packed settings-a holds (`dk_workbench_queries`, 8 bytes per entry, the method number in the fifth byte), by its place in the address map.
fn packed_queries(dev: Dev, file: &[u8]) -> Builder {
    let map: serde_json::Value = serde_json::from_slice(&read(&repo().join(format!("dist/modules/{}/settings-a.addrmap.json", dev.name)), "pnpm nx:pilot run test/settings-a:pack")).unwrap();
    let table = map["entries"].as_array().unwrap().iter().find(|e| e["name"] == "dk_workbench_queries").expect("settings-a declares queries");
    let (addr, size) = (table["addr"].as_u64().unwrap() as usize, table["size"].as_u64().unwrap() as usize);
    let base = be(file, dkneeds::H_IMAGE_OFF) as usize + addr;
    let mut methods = [0u8; 4];
    for i in 0..size / 8 {
        methods[i] = file[base + 8 * i + 4];
    }
    Builder::Queries { count: (size / 8) as u32, methods }
}

/// Load the packed settings-a on `dev` over `FakePlatform`, replaying the descriptor registration of its init; returns the loader and the MODULES instance word.
unsafe fn load_settings_a(dev: Dev) -> (Loader<FakePlatform, Card, DeviceRegistry>, [u8; 4]) {
    let file = packed(dev, "settings-a", "dkm", "test/settings-a");
    let offset = descriptor_offset(&file);
    let path = String::from_utf8(PATH[..PATH.len() - 1].to_vec()).unwrap();
    let mut card = Card::new(std::vec![(path, file)], dev.profile.id);
    card.init_calls.push(Builder::Workbench {
        offset,
        len: SCHEMA.len(),
    });
    card.init_calls.push(packed_queries(dev, &packed(dev, "settings-a", "dkm", "test/settings-a")));
    let mut loader = Loader::new(
        FakePlatform::new(dev.profile),
        card,
        DeviceRegistry {
            runtime: package_object(),
        },
    );
    loader.phase = Phase::Boot;
    assert_eq!(
        loader.load(PATH.as_ptr()),
        OK,
        "{}: load settings-a",
        dev.name
    );
    crate::hooks::command_run(CMD_MODULES, 1, null_mut(), [0].as_ptr(), 1);
    assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), OK);
    let body = (addr_of!(RBODY) as *const u8).add(1);
    assert_eq!(slice::from_raw_parts(body.add(16), 3), &[1, 1, 0]);
    assert_eq!(
        slice::from_raw_parts(body.add(19), b"test/settings-a\0".len()),
        b"test/settings-a\0"
    );
    let instance: [u8; 4] = slice::from_raw_parts(body.add(18 + 41), 4).try_into().unwrap();
    assert_ne!(instance, [0; 4]);
    (loader, instance)
}

#[test]
fn loaded_settings_fixture_publishes_its_packed_workbench_descriptor() {
    for dev in DEVICES {
        let file = packed(dev, "settings-a", "dkm", "test/settings-a");
        let offset = descriptor_offset(&file);
        let path = String::from_utf8(PATH[..PATH.len() - 1].to_vec()).unwrap();
        let mut card = Card::new(std::vec![(path, file)], dev.profile.id);
        card.init_calls.push(Builder::Workbench {
            offset,
            len: SCHEMA.len(),
        });

        let _guard = clean();
        let mut loader = Loader::new(
            FakePlatform::new(dev.profile),
            card,
            DeviceRegistry {
                runtime: package_object(),
            },
        );
        loader.phase = Phase::Boot;
        unsafe {
            assert_eq!(
                loader.load(PATH.as_ptr()),
                OK,
                "{}: load settings-a",
                dev.name
            );

            // The same MODULES entry Workbench uses to address DESCRIBE.
            assert!(crate::hooks::command_flags(CMD_MODULE_UI).is_some());
            crate::hooks::command_run(CMD_MODULES, 1, null_mut(), [0].as_ptr(), 1);
            assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), OK);
            let body = (addr_of!(RBODY) as *const u8).add(1);
            assert_eq!(slice::from_raw_parts(body.add(16), 3), &[1, 1, 0]);
            assert_eq!(
                slice::from_raw_parts(body.add(19), b"test/settings-a\0".len()),
                b"test/settings-a\0"
            );
            let instance = slice::from_raw_parts(body.add(18 + 41), 4);
            assert_ne!(instance, &[0; 4]);
            let mut describe = [0, 0, 0, 0, 0, 0];
            describe[2..].copy_from_slice(instance);
            crate::hooks::command_run(
                CMD_MODULE_UI,
                2,
                null_mut(),
                describe.as_ptr(),
                describe.len(),
            );
            assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), OK);
            assert_eq!(slice::from_raw_parts(body, SCHEMA.len()), SCHEMA);

            assert_eq!(loader.unload_body(0), OK, "{}: unload settings-a", dev.name);
            crate::hooks::command_run(
                CMD_MODULE_UI,
                3,
                null_mut(),
                describe.as_ptr(),
                describe.len(),
            );
            assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), E_NOT_FOUND);
        }
    }
}

/// Submit one MODULE_UI request for the loaded fixture, run the UI pass that performs it and poll the outcome: (status, value).
unsafe fn ui_request(instance: [u8; 4], ticket: u32, verb: u8, field: u8, revision: u32, value: &[u8], read_len: usize) -> (i32, u32, Option<Vec<u8>>) {
    let mut submit = std::vec![verb, 0];
    submit.extend_from_slice(&instance);
    submit.extend_from_slice(&ticket.to_be_bytes());
    submit.push(field);
    submit.extend_from_slice(&revision.to_be_bytes());
    submit.extend_from_slice(value);
    crate::hooks::command_run(CMD_MODULE_UI, 10, null_mut(), submit.as_ptr(), submit.len());
    let status = read_volatile(addr_of!(DKRT_UPLOAD_LAST));
    if status != OK {
        return (status, revision, None);
    }
    crate::package::module_ui::tick();
    let mut poll = std::vec![4u8, 0];
    poll.extend_from_slice(&instance);
    poll.extend_from_slice(&ticket.to_be_bytes());
    crate::hooks::command_run(CMD_MODULE_UI, 11, null_mut(), poll.as_ptr(), poll.len());
    assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), OK);
    let body = (addr_of!(RBODY) as *const u8).add(1);
    let reply = slice::from_raw_parts(body, 10 + read_len);
    assert_eq!(reply[0], 0, "the request is done");
    let result = i32::from_be_bytes(reply[1..5].try_into().unwrap());
    (result, u32::from_be_bytes(reply[5..9].try_into().unwrap()), (reply[9] != 0).then(|| reply[10..].to_vec()))
}

#[test]
fn the_packed_typed_settings_accept_a_valid_write_refuse_an_invalid_one_and_flag_a_bad_stored_value() {
    for dev in DEVICES {
        let _guard = clean();
        unsafe {
            // The settings client of a record is its index plus one. An open settings store without the stand-in module that `setup` puts in record 0: the fixture loads there.
            crate::settings::tests::setup(false);
            crate::settings::tests::teardown();
            let (mut loader, instance) = load_settings_a(dev);
            // the revision of a field comes from a read; one write that fits: gain 75 is stored and read back
            let (status, revision, stored) = ui_request(instance, 1, GET, GAIN, 0, b"", 0);
            assert_eq!((status, stored), (OK, None), "{}: gain unset", dev.name);
            let put = ui_request(instance, 2, PUT, GAIN, revision, &75i32.to_be_bytes(), 4);
            assert_eq!((put.0, put.2), (OK, Some(75i32.to_be_bytes().to_vec())), "{}: gain 75", dev.name);
            // typical refusals: above the maximum, and a choice index past the last label
            assert_eq!(ui_request(instance, 3, PUT, GAIN, put.1, &101i32.to_be_bytes(), 0).0, E_SET_VALUE, "{}: gain 101", dev.name);
            assert_eq!(ui_request(instance, 4, PUT, MODE, 0, &[3], 0).0, E_SET_VALUE, "{}: mode 3", dev.name);
            // the store takes any bytes: a stored index outside the choices reads as invalid
            let rec = (0..dkrt_abi::MAX_MODULES)
                .find(|&rec| crate::package::module_ui::instance(rec).to_be_bytes() == instance)
                .expect("the record of the loaded fixture");
            assert_eq!(crate::package::entry::module_put(rec as u32 + 1, b"mode\0".as_ptr(), [7u8].as_ptr(), 1), OK);
            assert_eq!(ui_request(instance, 5, GET, MODE, 0, b"", 0).0, E_SET_VALUE, "{}: stored mode 7", dev.name);
            assert_eq!(loader.unload_body(0), OK, "{}: unload settings-a", dev.name);
        }
    }
}

/// One query call for the loaded fixture and its poll after the UI pass: (runtime status, handler result, reply bytes).
unsafe fn query_call(instance: [u8; 4], ticket: u32, method: u8) -> (i32, i32, Vec<u8>) {
    let mut call = std::vec![5u8, 0];
    call.extend_from_slice(&instance);
    call.extend_from_slice(&ticket.to_be_bytes());
    call.push(method);
    crate::hooks::command_run(CMD_MODULE_UI, 10, null_mut(), call.as_ptr(), call.len());
    let status = read_volatile(addr_of!(DKRT_UPLOAD_LAST));
    if status != OK {
        return (status, 0, Vec::new());
    }
    crate::package::module_ui::tick();
    let mut poll = std::vec![4u8, 0];
    poll.extend_from_slice(&instance);
    poll.extend_from_slice(&ticket.to_be_bytes());
    crate::hooks::command_run(CMD_MODULE_UI, 11, null_mut(), poll.as_ptr(), poll.len());
    assert_eq!(read_volatile(addr_of!(DKRT_UPLOAD_LAST)), OK);
    let body = (addr_of!(RBODY) as *const u8).add(1);
    let head = slice::from_raw_parts(body, 10);
    assert_eq!(head[0], 0, "the call is done");
    (i32::from_be_bytes(head[1..5].try_into().unwrap()), i32::from_be_bytes(head[5..9].try_into().unwrap()), std::vec![head[9]])
}

#[test]
fn the_packed_settings_fixture_registers_its_query_table_in_init() {
    for dev in DEVICES {
        let _guard = clean();
        unsafe {
            crate::settings::tests::setup(false);
            crate::settings::tests::teardown();
            let file = packed(dev, "settings-a", "dkm", "test/settings-a");
            let Builder::Queries { count, methods } = packed_queries(dev, &file) else { unreachable!() };
            assert_eq!((count, methods[0]), (1, 1), "{}: the packed table is the one method BootAtInit", dev.name);
            // the load fails when init's registration is refused, so loading is the proof that the table was accepted (the call itself runs ColdFire code: device-only)
            let (mut loader, instance) = load_settings_a(dev);
            let (runtime, handler, reply) = query_call(instance, 1, 1);
            assert_eq!((runtime, handler, reply), (OK, 0, std::vec![0xAB]), "{}: method 1 reaches the registered table", dev.name);
            assert_eq!(query_call(instance, 2, 2).0, dkrt_sdk::proto::E_MODULE_UI_METHOD, "{}: no method 2", dev.name);
            assert_eq!(loader.unload_body(0), OK, "{}: unload settings-a", dev.name);
        }
    }
}
