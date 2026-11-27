use super::*;
use crate::transport::{DKRT_UPLOAD_LAST, RBODY};
use core::ptr::{addr_of, null_mut, read_volatile};
use core::slice;
use dkrt_sdk::proto::{CMD_MODULES, CMD_MODULE_UI};

const PATH: &[u8] = b"modules/settings-a.dkm\0";
// V1 exposes one immediate text setting, boot, of up to 16 bytes.
const SCHEMA: &[u8] = b"\x01\x0dBoot settings\x01\x04boot\x0aBoot value\x10\x00\x00";

fn descriptor_offset(file: &[u8]) -> usize {
    let image_off = be(file, dkneeds::H_IMAGE_OFF) as usize;
    let image_size = be(file, dkneeds::H_IMAGE_SIZE) as usize;
    let image = &file[image_off..image_off + image_size];

    // The pack has no symbol for the generated static array. Locate the
    // fixture's expected V1 descriptor in the packed image.
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
