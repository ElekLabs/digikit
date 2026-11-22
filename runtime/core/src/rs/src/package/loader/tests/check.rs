//! The checks of a file before anything is allocated, and the role word.
use super::*;

#[test]
fn the_role_word_picks_the_steps() {
    type Steps = RoleSteps<FakePlatform, FakeImages, FakeRegistry>;
    assert!(Steps::of(1).check as usize == Steps::SERVICE.check as usize);
    assert!(Steps::of(0).check as usize == Steps::MODULE.check as usize);
    // a word that is neither is a module, as the loader always took it
    assert!(Steps::of(7).check as usize == Steps::MODULE.check as usize);
}

#[test]
fn no_load_starts_once_the_tables_were_marked_bad() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _reset = TableBadGuard;
        let mut l = loader(Init::Ok);
        *core::ptr::addr_of_mut!(crate::package::invariants::DKRT_TABLE_BAD) = 1;
        assert_eq!(l.load(PATH.as_ptr()), E_TABLE);
        assert!(l.m.log.is_empty(), "the file is not even read");
        assert_eq!(st().modules[0].used, 0);
    }
}

#[test]
fn an_import_name_without_a_nul_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let name = vec![b'x'; dkneeds::CF_IMPORT_LEN as usize];
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[&name]));
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        assert_eq!(l.missing()[0], 0);
    }
}

#[test]
fn a_bad_relocation_a_register_section_outside_the_file_and_a_bad_keys_table_are_refused_before_anything_is_allocated() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a relocation past the last word of the image, and one of an unknown kind
        for relocs in [[(30u32, 0u32)], [(8, 2 << 24)]] {
            let mut l = loader_of(Init::Ok, module_file_with(&relocs, &[]));
            assert_eq!(l.load(PATH.as_ptr()), E_FORMAT, "{relocs:?}");
            assert_eq!(l.m.allocs, 0);
        }
        // the keys table lies outside the file
        let mut f = module_file();
        let end = f.len() as u32;
        put(&mut f, dkneeds::H_REG_OFF, end + 16);
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // the keys table has another magic: a file of the format before the table
        let mut f = module_file();
        let at = f.len() as u32 - 8;
        put(&mut f, at, 0);
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_CONTRACT);
        assert_eq!(l.m.allocs, 0);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_provider_record_in_use_and_a_second_module_of_the_same_name_are_refused_as_loaded() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        let allocs = l.m.allocs;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_LOADED);
        assert_eq!(l.m.allocs, allocs, "the second load takes nothing");
        assert_eq!(records::module(MAX_MODULES).used, 1, "the first one stays");

        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let allocs = l.m.allocs;
        assert_eq!(l.load(PATH.as_ptr()), E_LOADED);
        assert_eq!((l.m.allocs, l.m.live.len()), (allocs, 1));
        assert_eq!((st().modules[0].used, st().modules[1].used, st().loads), (1, 0, 1));
    }
}
