//! The port file (role 2): the check, the copies of its sites and the firmware imports of a service file.
use super::*;

#[test]
fn a_port_file_loads_as_a_provider_copies_its_tail_sites_and_unloads_clean() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = port_loader(port_file());
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), OK);
        let rec = MAX_MODULES;
        let base = records::module(rec).base;
        assert_eq!((records::module(rec).used, l.roles[rec], st().loads), (1, 2, 0));
        // the image block (32 bytes of image and 12 of bss) and the table copy
        assert_eq!(l.m.live.len(), 2);
        // the copy of the site follows the image: the stock bytes, then `jmp.l` behind the site
        let mem = core::slice::from_raw_parts(l.m.live[0].1.as_ptr() as *const u8, 44);
        assert_eq!(&mem[..2], [0x4e, 0x75], "the return stub");
        assert_eq!(&mem[32..38], STOCK);
        assert_eq!(&mem[38..40], [0x4e, 0xf9]);
        assert_eq!(rd32(mem.as_ptr().add(40)), SITE + 6);
        assert!(l.p.calls.borrow().contains(&crate::fake_platform::Call::Flush { addr: base + 32, len: 12 }), "the copy is written back");
        // the port file installs nothing and publishes nothing
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        assert_eq!(site_bytes(&l, 6), STOCK);
        // taken down, it gives both blocks back
        assert_eq!(adopt(&mut l, &[rec]).unload(rec), OK);
        assert!(l.m.live.is_empty());
        assert_eq!(records::module(rec).used, 0);
    }
}

#[test]
fn a_port_file_for_another_firmware_or_over_other_stock_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // another firmware, an image with no fw_id
        let mut other = FW_ID;
        other[0] ^= 1;
        for id in [Some(other), None] {
            let mut l = port_loader(port_file());
            set_fw_id(&l, id);
            assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), E_HOOK_STOCK, "image id {id:?}");
            assert_eq!(l.m.allocs, 0);
        }
        // the live bytes of the copied site are not the stock ones: the check value names them
        let mut l = port_loader(port_file());
        l.p.put_code(SITE + 1, &[0x99]);
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), E_HOOK_STOCK);
        assert_eq!(l.m.allocs, 0);
        // a raw site is not copied: its bytes are not read at the load of the port file (the installer checks them)
        let mut l = port_loader(port_file());
        l.p.put_code(0x400b_0000, &[0x77; 6]);
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), OK);
    }
}

#[test]
fn a_port_file_that_does_not_fit_its_image_or_its_sites_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the bss is smaller than the copies need
        let mut l = port_loader(port_file_with(11, |_| {}));
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // a site of 5 bytes (not whole instructions) and one of 4 (no room for jmp.l)
        for len in [5u16, 4] {
            let mut l = port_loader(port_file_with(12, |s| s[1].len = len));
            assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), E_RANGE, "len {len}");
            assert_eq!(l.m.allocs, 0);
        }
        // the image does not start with `rts`
        let mut f = port_file();
        f[IMAGE_OFF] = 0;
        seal(&mut f);
        let mut l = port_loader(f);
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // a port file only loads into a provider record, and a service file never takes a port file's role
        let mut l = port_loader(port_file());
        assert_eq!(l.load(PORT_PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
    }
}

#[test]
fn the_firmware_imports_of_a_service_file_resolve_through_the_port_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let names = [
            fw_import("dk_fw_", NAME_H),           // an address
            fw_import("dk_fw_", "cf.other"),       // absent: 0
            fw_import("dk_exit_", HOOK_H) + "_0",  // the end of the site
            fw_import("dk_tail_", HOOK_H),         // the copy
            fw_import("dk_stock_", HOOK_H),        // the same copy
            fw_import("dk_ret_", HOOK_H),          // the return stub
            fw_import("dk_ret_", "hook.gone"),     // an absent site: 0
        ];
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let mut l = port_loader(port_file());
        l.m.files.insert(IMP_PATH[..IMP_PATH.len() - 1].to_vec(), importing_service(&refs));
        assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), OK);
        let port_base = records::module(MAX_MODULES).base;
        assert_eq!(l.load_provider(IMP_PATH.as_ptr(), 1), OK);
        // the service's image block is the one allocated for provider 1: the third block (port image, port table, service image)
        let mem = core::slice::from_raw_parts(l.m.live[2].1.as_ptr() as *const u8, 32);
        let word = |i: usize| rd32(mem.as_ptr().add(4 + 4 * i));
        assert_eq!(word(0), 0x1234 + 4);
        assert_eq!(word(1), 4, "an absent name is 0 plus the addend");
        assert_eq!(word(2), SITE + 6 + 4);
        assert_eq!(word(3), port_base + 32 + 4);
        assert_eq!(word(4), port_base + 32 + 4);
        assert_eq!(word(5), port_base + 4);
        assert_eq!(word(6), 4);
    }
}

#[test]
fn a_firmware_import_the_port_file_does_not_list_or_that_has_no_port_file_refuses_the_service_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a name the port file lacks, and a tail of a raw site (it has no copy)
        for bad in [fw_import("dk_fw_", "cf.unknown"), fw_import("dk_tail_", "hook.raw"), fw_import("dk_ret_", "hook.unknown")] {
            let mut l = port_loader(port_file());
            l.m.files.insert(IMP_PATH[..IMP_PATH.len() - 1].to_vec(), importing_service(&[&bad]));
            assert_eq!(l.load_provider(PORT_PATH.as_ptr(), 0), OK);
            let allocs = l.m.allocs;
            assert_eq!(l.load_provider(IMP_PATH.as_ptr(), 1), E_IMPORT, "{bad}");
            assert_eq!(l.m.allocs, allocs, "refused before anything is allocated");
            assert_ne!(l.import_hash, 0, "the hash of the missing name is kept for the log");
        }
        // no port file loaded: the same import is refused
        let mut l = port_loader(port_file());
        l.m.files.insert(IMP_PATH[..IMP_PATH.len() - 1].to_vec(), importing_service(&[&fw_import("dk_fw_", NAME_H)]));
        assert_eq!(l.load_provider(IMP_PATH.as_ptr(), 1), E_IMPORT);
        assert_eq!(l.m.allocs, 0);
        // a module with such an import is not a service file: the port file is not asked, and the name stays unknown
        let mut f = module_file_with(&[(8, 1 << 24)], &[fw_import("dk_fw_", NAME_H).as_bytes()]);
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(l.m.allocs, 0);
    }
}
