use super::*;

const SETTINGS_PAGE: Fixture = Fixture { leaf: "settings-page", nx: "test/settings-page" };

fn imports_of(file: &[u8]) -> Vec<String> {
    let (off, n) = (be(file, dkneeds::H_IMPORT_OFF) as usize, be(file, dkneeds::H_IMPORT_N) as usize);
    (0..n)
        .map(|i| {
            let at = off + i * dkneeds::CF_IMPORT_LEN as usize;
            let name = &file[at..at + dkneeds::CF_IMPORT_LEN as usize];
            String::from_utf8_lossy(&name[..name.iter().position(|b| *b == 0).unwrap()]).to_string()
        })
        .collect()
}

/// A module with a settings page from `<dk/settings/page.h>`: the packed file links the page component over existing imports only (the settings store and the rows
/// and the choice of core/ui), has a `menu` entry, builds no menu and takes no SETTINGS row of its own (so the loader can reload it). Its imports resolve in an image with core/ui;
/// an image without core/ui refuses it.
#[test]
fn a_settings_page_module_links_the_component_over_existing_imports_and_needs_core_ui() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let file = packed(*dev, SETTINGS_PAGE.leaf, "dkm", SETTINGS_PAGE.nx);
        let imports = imports_of(&file);
        for want in [
            "dk_settings_open",
            "dk_settings_get",
            "dk_settings_put",
            "dk_ui_open",
            "dk_ui_menu_add_toggle",
            "dk_ui_menu_add_value",
            "dk_ui_menu_add_action",
            "dk_ui_menu_refresh",
            "dk_ui_choice",
        ] {
            assert!(imports.iter().any(|i| i == want), "{}: the packed page does not import {want}", dev.name);
        }
        for refused in ["dk_ui_menu_new", "dk_ui_settings_row"] {
            assert!(!imports.iter().any(|i| i == refused), "{}: a page module that imports {refused} could not be reloaded", dev.name);
        }
        assert_ne!(be(&file, dkneeds::H_MENU), 0, "{}: no menu entry", dev.name);
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&SETTINGS_PAGE], &SERVICES);
            let _run = boot_run(&mut l, &plan, &[&SETTINGS_PAGE], true);
            assert!(imports_resolve(&mut l, &image, &file, "settings-page"), "{}: the image has core/ui", dev.name);
            let bare = Image { entries: Vec::new(), modules: Vec::new() };
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| imports_resolve(&mut l, &bare, &file, "settings-page")));
            assert!(refused.is_err(), "{}: an image without core/ui accepted the page's dk_ui_* imports", dev.name);
        }
        let _ = clean_state();
    }
}
