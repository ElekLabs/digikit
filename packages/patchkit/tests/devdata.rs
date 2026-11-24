//! Device data: the constants `patchkit gen` writes from the device files agree with them (the files are committed, and a
//! stale one fails here as it fails `patchkit gen --check`), a fact that devices disagree on is refused for the shared runtime,
//! and the device list of the code is the device files' list.

use patchkit::devdata;
use patchkit::devgen::{self, Facts};
use patchkit::modgen;
use patchkit::module::Library;
use patchkit::port::Registry;
use patchkit::typed::Models;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_generated_file_is_current() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let models = Models::from_dir(&root().join("devices")).unwrap();
    lib.lower(&models).unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let mut n = 0;
    for m in &lib.modules {
        let mut files = modgen::generate(&lib, &models, m.id()).unwrap();
        files.extend(devgen::generate(&lib, &facts, m.id()).unwrap());
        for (rel, text) in files {
            let path = m.dir.join(&rel);
            let have = std::fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(
                have,
                text,
                "{} is stale: run `patchkit gen`",
                path.display()
            );
            n += 1;
        }
    }
    assert!(n > 20, "only {n} generated files");
}

#[test]
fn the_runtime_constants_are_the_device_files_numbers() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let files = devgen::generate(&lib, &facts, "core/runtime").unwrap();
    let get = |p: &str| &files.iter().find(|f| f.0 == p).unwrap().1;
    // the device object `dkrt_dev` of each device: the numbers of devices/*.toml [board] and [digikit], in the order of struct DkrtDevice
    let objects: Vec<Vec<String>> = files
        .iter()
        .filter(|f| f.0.starts_with("build/gen/dkdv-d"))
        .map(|f| {
            f.1.lines()
                .filter_map(|l| l.trim().strip_prefix(".long "))
                .map(|l| l.split_whitespace().next().unwrap().to_string())
                .collect()
        })
        .collect();
    let common = [
        "0x444b4456", // magic
        "0x1",        // version
        "0x150", // size: the header words, the facts, `tracks`, the 64 key codes and the four key flag bits
        "0xf",   // caps: DSP, store, eSDHC, safe key
        "0xec070000", // console UART
        "0x48000000", // uncached alias
        "0x40000000", // cached base
        "0x8000000", // DDR bytes
        "0xfc0cc000", // eSDHC
        "0x13",  // safe key
        "0x47fe0000", // keep base
        "0x1000", // keep bytes
        "0xa2c2b", // DSP cycles in a block
        "0x10",  // first run-time machine type
        "0x8",   // run-time machine types
        "0x10",  // tracks
    ];
    // both devices have the same numbers today (the key codes after them differ: `the_key_map_...`)
    assert_eq!(objects.len(), 2);
    assert!(
        objects
            .iter()
            .all(|o| o.len() == 0x150 / 4 && o[..common.len()] == common),
        "{objects:?}"
    );
    let offsets = get("build/gen/dkdv-offsets.inc");
    for want in [
        ".equ DKDV_CAPS, 12",
        ".equ DKDV_DDR_UNCACHED_ALIAS, 20",
        ".equ DKDV_DYN_TYPES, 56",
    ] {
        assert!(offsets.contains(want), "{want}");
    }
    let dn2 = get("build/gen/device-dn2.inc");
    assert!(dn2.contains(".equ DEVICE_ID, 0x2 "), "{dn2}");
    // the effect slots and the DSP area are read by other modules: their generated files hold them
    let dynfx = devgen::generate(&lib, &facts, "core/dynfx").unwrap();
    let dynfx = |p: &str| &dynfx.iter().find(|f| f.0 == p).unwrap().1;
    assert!(
        dynfx("build/gen/device-dn2.inc").contains(".equ LIMIT_FX_SLOTS, 0x3 "),
        "dn2"
    );
    assert!(
        dynfx("build/gen/device-dt2.inc").contains(".equ LIMIT_FX_SLOTS, 0x3 "),
        "dt2"
    );
    let link = devgen::generate(&lib, &facts, "core/dsp-link").unwrap();
    let link = |p: &str| &link.iter().find(|f| f.0 == p).unwrap().1;
    // the store area of each device ([card.store], docs/design/digikit-store.md): DN2 from 0x800000, DT2 after the +Drive, both to the end of the card
    for (inc, first, sectors) in [
        (dn2, "0x800000", "0x261a000"),
        (get("build/gen/device-dt2.inc"), "0x2df1000", "0x29000"),
    ] {
        assert!(
            inc.contains(&format!(".equ STORE_FIRST, {first} ")),
            "{inc}"
        );
        assert!(
            inc.contains(&format!(".equ STORE_SECTORS, {sectors} ")),
            "{inc}"
        );
    }
    // 0x50000 bytes
    let dl = link("build/gen/device-dn2.inc");
    assert!(dl.contains("#define DL_AREA_BYTES 327680\n"), "{dl}");
    // the PEEK ranges are per device (the runtime's region differs), up to four, unused ones 0, 0
    assert!(dn2.contains(".equ PEEK_N, 0x2 "), "{dn2}");
    assert!(dn2.contains(".equ PEEK_LO_1, 0x46712000 "), "{dn2}");
    assert!(dn2.contains(".equ PEEK_HI_3, 0x0 "), "{dn2}");
    let dt2 = get("build/gen/device-dt2.inc");
    assert!(
        dt2.contains(".equ PEEK_LO_1, 0x47e8b000 ") && dt2.contains(".equ PEEK_HI_1, 0x47fe1000 "),
        "{dt2}"
    );
    // the profiling clock: DTIM0 DTCN0 at 132 MHz on both devices
    for inc in [dn2, dt2] {
        assert!(inc.contains(".equ SPAN_CLOCK, 0xfc07000c "), "{inc}");
        assert!(inc.contains(".equ SPAN_CLOCK_HZ, 0x7de2900 "), "{inc}");
    }
}

#[test]
fn a_device_without_a_span_clock_gets_zero() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let text = std::fs::read_to_string(root().join("devices/digitone-ii.toml")).unwrap();
    let text: String = text
        .lines()
        .filter(|l| !l.starts_with("span_clock"))
        .map(|l| format!("{l}\n"))
        .collect();
    let facts = Facts::from_texts([text.as_str()]).unwrap();
    let files = devgen::generate(&lib, &facts, "core/runtime").unwrap();
    let inc = &files
        .iter()
        .find(|f| f.0 == "build/gen/device-dn2.inc")
        .unwrap()
        .1;
    assert!(inc.contains(".equ SPAN_CLOCK, 0x0 "), "{inc}");
    assert!(inc.contains(".equ SPAN_CLOCK_HZ, 0x0 "), "{inc}");
}

/// What `devgen::generate` makes of `module` when its `[[gen]]` entries are replaced by one entry.
fn generate_with(
    module: &str,
    kind: &str,
    path: &str,
    uses: &[&str],
) -> Option<patchkit::error::Result<Vec<(String, String)>>> {
    let mut lib = Library::scan_repo_or_skip(&root())?;
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let m = lib.modules.iter_mut().find(|m| m.id() == module).unwrap();
    m.manifest.gens = vec![patchkit::module::GenDef {
        kind: kind.into(),
        path: path.into(),
        uses: uses.iter().map(|s| s.to_string()).collect(),
    }];
    Some(devgen::generate(&lib, &facts, module))
}

#[test]
fn a_uses_list_names_constants_the_module_reads() {
    let gas = "build/gen/x-{dev}.inc";
    // a name that is no constant
    if let Some(r) = generate_with("core/rows", "gas", gas, &["NO_SUCH_*", "DEVICE_ID"]) {
        assert!(r.unwrap_err().0.contains("NO_SUCH_*"));
    }
    // a constant that the sources of the module never use
    if let Some(r) = generate_with("core/rows", "gas", gas, &["DEVICE_ID", "PANEL_ENCODERS"]) {
        let e = r.unwrap_err().0;
        assert!(
            e.contains("PANEL_ENCODERS") && !e.contains("DEVICE_ID,"),
            "{e}"
        );
        // the message names only the unreferenced constants, so the used one is absent at any position
        assert!(!e.contains("DEVICE_ID"), "{e}");
    }
    // an empty list on a kind that needs one
    if let Some(r) = generate_with("core/rows", "gas", gas, &[]) {
        let e = r.unwrap_err().0;
        assert!(e.contains("needs `uses`"), "{e}");
    }
    // a list on a kind that takes none
    if let Some(r) = generate_with(
        "core/sample-pool",
        "gas-sample-pool",
        "build/gen/sample-pool-{dev}.inc",
        &["SP_DDR_BASE"],
    ) {
        let e = r.unwrap_err().0;
        assert!(e.contains("takes no `uses`"), "{e}");
    }
    // a trailing `*` takes the whole family and nothing else
    if let Some(r) = generate_with("core/rows", "gas", gas, &["ROWS_B1_*"]) {
        for (_, text) in r.unwrap() {
            let names: Vec<&str> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix(".equ "))
                .map(|l| l.split(',').next().unwrap())
                .collect();
            assert_eq!(names.len(), 3, "{text}");
            assert!(names.iter().all(|n| n.starts_with("ROWS_B1_")), "{text}");
        }
    }
}

#[test]
fn peek_and_keep_ranges_are_checked() {
    let dt2 = std::fs::read_to_string(root().join("devices/digitakt-ii.toml")).unwrap();
    let dn2 = std::fs::read_to_string(root().join("devices/digitone-ii.toml")).unwrap();
    for (what, bad) in [
        (
            "an empty range",
            dt2.replace("[0x40000000, 0x47e30000]", "[0x40000000, 0x40000000]"),
        ),
        (
            "an unaligned range",
            dt2.replace("[0x40000000, 0x47e30000]", "[0x40000001, 0x47e30000]"),
        ),
        (
            "five ranges",
            dt2.replace("peek = [", "peek = [[4, 8], [8, 12], [12, 16], [16, 20], "),
        ),
        (
            "a keep area that is not a page multiple",
            dt2.replace("keep_bytes = 0x1000", "keep_bytes = 0x800"),
        ),
    ] {
        assert_ne!(bad, dt2, "{what}: the test text did not change");
        assert!(
            Facts::from_texts([bad.as_str(), dn2.as_str()]).is_err(),
            "{what} is accepted"
        );
    }
}

#[test]
fn a_fact_that_devices_disagree_on_becomes_data_of_the_device_object() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let dt2 = std::fs::read_to_string(root().join("devices/digitakt-ii.toml")).unwrap();
    let dn2 = std::fs::read_to_string(root().join("devices/digitone-ii.toml")).unwrap();
    // move the Digitone's safe key: the runtime is one image, so the device object carries the difference and `gen` accepts it
    let moved = dn2.replace("safe_key = 19", "safe_key = 20");
    assert_ne!(moved, dn2);
    let facts = Facts::from_texts([dt2.as_str(), moved.as_str()]).unwrap();
    let files = devgen::generate(&lib, &facts, "core/runtime").unwrap();
    let mut keys: Vec<String> = files
        .iter()
        .filter(|f| f.0.starts_with("build/gen/dkdv-d"))
        .map(|f| {
            let l = f.1.lines().find(|l| l.contains("safe_key:")).unwrap();
            l.trim().split_whitespace().nth(1).unwrap().to_string()
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["0x13", "0x14"]);
}

#[test]
fn the_code_knows_the_devices_of_the_device_files() {
    // every port names a device the code knows, and the limits a port records are the device's `[patch]` limits
    let reg = Registry::from_dir(&root().join("ports")).unwrap();
    assert!(!reg.ports.is_empty());
    for p in &reg.ports {
        let d = devdata::by_short(&p.port.device).unwrap_or_else(|| panic!("{}", p.id()));
        assert_eq!(p.limits.container_max, d.container_max, "{}", p.id());
        assert_eq!(p.limits.dsp_stored_max, d.dsp_stored_max, "{}", p.id());
        assert_eq!(p.limits.dsp_depacked_max, d.dsp_depacked_max, "{}", p.id());
        assert_eq!(p.limits.main_staged_max, d.main_staged_max, "{}", p.id());
    }
    // ids are unique per kind
    let all = devdata::all();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!((a.syx_id, a.digikit_id), (b.syx_id, b.digikit_id));
            assert!(a.digikit_id.is_none() || a.digikit_id != b.digikit_id);
        }
    }
}

fn device_texts() -> (String, String) {
    (
        std::fs::read_to_string(root().join("devices/digitakt-ii.toml")).unwrap(),
        std::fs::read_to_string(root().join("devices/digitone-ii.toml")).unwrap(),
    )
}

#[test]
fn a_limit_above_the_contract_maximum_is_refused() {
    let (dt2, dn2) = device_texts();
    // the committed files are within the maxima
    Facts::from_texts([dt2.as_str(), dn2.as_str()]).unwrap();
    for (what, from, to) in [
        ("modules", "modules = 8", "modules = 9"),
        ("fx_slots", "fx_slots = 3", "fx_slots = 5"),
        ("send_slots", "send_slots = 2", "send_slots = 3"),
        ("param_rows", "param_rows = 16", "param_rows = 17"),
        (
            "dsp_area_bytes",
            "dsp_area_bytes = 524288",
            "dsp_area_bytes = 524292",
        ),
    ] {
        let bad = dt2.replace(from, to);
        assert_ne!(bad, dt2, "{what}: the test text did not change");
        let e = Facts::from_texts([bad.as_str(), dn2.as_str()])
            .unwrap_err()
            .0;
        assert!(
            e.contains(what) && e.contains("contract maximum"),
            "{what}: {e}"
        );
    }
    let bad = dn2.replace("page_rows = 8", "page_rows = 9");
    assert!(Facts::from_texts([dt2.as_str(), bad.as_str()]).is_err());
    // a value below the maximum is a device choice and is accepted
    let less = dt2.replace("fx_slots = 3", "fx_slots = 2");
    Facts::from_texts([less.as_str(), dn2.as_str()]).unwrap();
}

#[test]
fn a_send_page_cell_needs_a_parameter_row() {
    let (first, second) = device_texts();
    // fewer reserved rows than send slots: a cell would have no row
    let bad = first.replace("send_rows = 2", "send_rows = 1");
    assert_ne!(bad, first);
    let e = Facts::from_texts([bad.as_str(), second.as_str()])
        .unwrap_err()
        .0;
    assert!(e.contains("send_rows") && e.contains("send_slots"), "{e}");
    // as many rows as slots is a device choice (the second device has one slot: one row is enough)
    let one = second.replace("send_rows = 2", "send_rows = 1");
    assert_ne!(one, second);
    Facts::from_texts([first.as_str(), one.as_str()]).unwrap();
}

#[test]
fn machine_rows_are_the_run_time_machine_types_of_types_toml() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let (dt2, dn2) = device_texts();
    let bad = dt2.replace("machine_rows = 8", "machine_rows = 7");
    assert_ne!(bad, dt2);
    let facts = Facts::from_texts([bad.as_str(), dn2.as_str()]).unwrap();
    let e = devgen::generate(&lib, &facts, "core/runtime")
        .unwrap_err()
        .0;
    assert!(
        e.contains("machine_rows") && e.contains("types.toml"),
        "{e}"
    );
}

#[test]
fn the_free_effect_rows_are_the_devices_fx_slots() {
    // core/dynfx reserves one free row of the effect chain for each of the device's effect slots (docs/design/digikit-dkm.md); both devices have it
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let dynfx = lib.get("core/dynfx").unwrap();
    let rows = dynfx
        .manifest
        .entry
        .iter()
        .filter(|e| e.registry == "fx")
        .count() as u64;
    for file in ["digitakt-ii", "digitone-ii"] {
        let t: toml::Value = toml::from_str(
            &std::fs::read_to_string(root().join(format!("devices/{file}.toml"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rows,
            t["limits"]["fx_slots"].as_integer().unwrap() as u64,
            "{file}"
        );
    }
    assert!(
        dynfx
            .manifest
            .module
            .ports
            .iter()
            .all(|p| p.starts_with("dt2-") || p.starts_with("dn2-"))
    );
}

#[test]
fn the_effect_page_rows_are_eight_parameter_rows_for_each_fx_slot() {
    // the effect pages take 7 knobs and MIX for each effect slot; the device file's [fx] has a value index for each of them (docs/design/digikit-registry.md "Effect pages")
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = lib.get("core/services/fx").unwrap();
    // the rows themselves are core/rows' (the count of each block against the device file: tests/rows.rs)
    assert!(
        m.manifest.entry.iter().all(|e| e.registry != "params"),
        "the fx service reserves no parameter rows"
    );
    // (device file, the number of stock pages: the firmware's page accessor has that many rows, the effect pages follow them, the array's halfwords: 71 / 101)
    for (file, first, array_hw) in [("digitakt-ii", 33, 71), ("digitone-ii", 37, 101)] {
        let t: toml::Value = toml::from_str(
            &std::fs::read_to_string(root().join(format!("devices/{file}.toml"))).unwrap(),
        )
        .unwrap();
        let slots = t["limits"]["fx_slots"].as_integer().unwrap() as u64;
        let rows = 8 * slots;
        let send_rows = t["limits"]["send_rows"].as_integer().unwrap() as u64;
        let idx = t["fx"]["value_indices"].as_array().unwrap();
        assert!(
            idx.len() as u64 >= rows,
            "{file}: a value index for each parameter row"
        );
        assert_eq!(t["fx"]["page_first"].as_integer(), Some(first), "{file}");
        // the send page (finding 64): a device that has one lists the Sound indices of its cells and a Sound layout that ends before the Fx array; its cells are parameter rows of core/rows
        if let Some(track_page) = t["fx"].get("track_page").and_then(|v| v.as_integer()) {
            let send = t["limits"]["send_slots"].as_integer().unwrap() as u64;
            let n = t["fx"]["send_indices"].as_array().unwrap().len() as u64;
            // the device reserves send_rows rows (both devices the same number); it uses n of them
            assert!(
                track_page > 0 && send <= n && n <= send_rows,
                "{file}: {send} {n} {send_rows}"
            );
            let (off, stride) = (
                t["fx"]["sound_off"].as_integer().unwrap(),
                t["fx"]["sound_stride"].as_integer().unwrap(),
            );
            assert_eq!(
                off + 16 * stride,
                t["fx"]["array_off"].as_integer().unwrap(),
                "{file}"
            );
        }
        // the values and the mark are indices of the array that no stock parameter uses; the u32 behind the array
        assert_eq!(
            t["fx"]["tail_off"].as_integer(),
            Some(2 * array_hw),
            "{file}"
        );
        let mark = t["fx"]["mark_index"].as_integer().unwrap();
        assert!(mark < array_hw, "{file}");
        assert!(
            idx.iter().all(|i| i.as_integer().unwrap() < array_hw),
            "{file}"
        );
        // persistence: the kit's spare storage halfwords hold the values and one ID for each row (finding 61), inside the 0xa0-byte record, after its 46 used halfwords and before its u32 at 0x98
        let off = t["fx"]["store_off"].as_integer().unwrap() as usize;
        let hw = t["fx"]["store_halfwords"].as_integer().unwrap() as usize;
        assert!(idx.len() + idx.len() / 8 <= hw, "{file}");
        assert!(off >= 4 + 2 * 46 && off + 2 * hw <= 0x98, "{file}");
    }
}

/// The device facts the fx service binds its table from (`services/fx/src/fxpage/tables.rs`) are the device files' `[fx]` and `[limits]`, read here from the TOML and not from the
/// generator's own constants: one entry for each device that has effect pages, in the order of the file.
#[test]
fn the_fx_service_tables_are_the_device_files_fx_facts() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let files = devgen::generate(&lib, &facts, "core/services/fx").unwrap();
    let text = &files
        .iter()
        .find(|f| f.0.ends_with("fxpage/tables.rs"))
        .expect("the fx service generates its device facts")
        .1;
    let mut n = 0;
    for file in ["digitakt-ii", "digitone-ii"] {
        let t: toml::Value = toml::from_str(
            &std::fs::read_to_string(root().join(format!("devices/{file}.toml"))).unwrap(),
        )
        .unwrap();
        let int = |tab: &str, key: &str| t[tab][key].as_integer().unwrap();
        let id = int("digikit", "id");
        // the entry of this device
        let at = text
            .find(&format!("device: {id},"))
            .unwrap_or_else(|| panic!("{file}: no entry\n{text}"));
        let entry = &text[at..at + text[at..].find("},").unwrap()];
        let field = |name: &str| -> String {
            entry
                .lines()
                .find_map(|l| l.trim().strip_prefix(&format!("{name}: ")))
                .unwrap_or_else(|| panic!("{file}: no {name}\n{entry}"))
                .trim_end_matches(',')
                .to_string()
        };
        let hex = |v: i64| format!("{v:#x}");
        let idx: Vec<i64> = t["fx"]["value_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_integer().unwrap())
            .collect();
        assert_eq!(
            field("page_first"),
            int("fx", "page_first").to_string(),
            "{file}"
        );
        assert_eq!(
            field("slots"),
            int("limits", "fx_slots").to_string(),
            "{file}"
        );
        assert_eq!(field("array_off"), hex(int("fx", "array_off")), "{file}");
        assert_eq!(field("values_n"), idx.len().to_string(), "{file}");
        assert_eq!(field("kit_ptr"), hex(int("fx", "kit_ptr")), "{file}");
        assert_eq!(
            field("track_page"),
            int("fx", "track_page").to_string(),
            "{file}"
        );
        assert_eq!(
            field("send_template"),
            hex(int("fx", "send_template")),
            "{file}"
        );
        assert_eq!(
            field("sound_stride"),
            hex(int("fx", "sound_stride")),
            "{file}"
        );
        assert_eq!(field("store_off"), hex(int("fx", "store_off")), "{file}");
        let sends = t["fx"]["send_indices"].as_array().unwrap();
        assert_eq!(
            field("send_cells"),
            int("limits", "send_slots").to_string(),
            "{file}"
        );
        assert!(
            field("send_index").starts_with(&format!("[{}", sends[0].as_integer().unwrap())),
            "{file}"
        );
        // the value indices, padded with 0 to 32 entries
        let want: Vec<String> = (0..32)
            .map(|i| idx.get(i).copied().unwrap_or(0).to_string())
            .collect();
        assert_eq!(
            field("value_indices"),
            format!("[{}]", want.join(", ")),
            "{file}"
        );
        n += 1;
    }
    assert_eq!(n, text.matches("DeviceFacts {").count(), "{text}");
}

#[test]
fn the_reserved_parameter_and_page_rows_are_the_profile_limits() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let dynreg = lib.get("core/rows").unwrap();
    let count = |registry: &str, dev: &str| {
        dynreg
            .manifest
            .entry
            .iter()
            .filter(|e| {
                e.registry == registry
                    && (e.devices.is_empty() || e.devices.iter().any(|d| d == dev))
            })
            .count() as u64
    };
    let dyndsp = lib.get("core/dyndsp").unwrap();
    let voices = |dev: &str| {
        dyndsp
            .manifest
            .entry
            .iter()
            .filter(|e| {
                e.registry == "voices"
                    && (e.devices.is_empty() || e.devices.iter().any(|d| d == dev))
            })
            .count() as u64
    };
    for (file, dev) in [("digitakt-ii", "dt2"), ("digitone-ii", "dn2")] {
        let t: toml::Value = toml::from_str(
            &std::fs::read_to_string(root().join(format!("devices/{file}.toml"))).unwrap(),
        )
        .unwrap();
        let l = &t["limits"];
        assert_eq!(
            voices(dev),
            l["machine_rows"].as_integer().unwrap() as u64,
            "{dev} voice rows of core/dyndsp"
        );
        assert_eq!(
            count("pages", dev),
            l["page_rows"].as_integer().unwrap() as u64,
            "{dev} page rows of core/rows"
        );
        assert_eq!(
            count("machines", dev),
            l["machine_rows"].as_integer().unwrap() as u64,
            "{dev} machine rows of core/rows"
        );
    }
}

#[test]
fn the_link_width_is_a_shared_c_header_from_the_contract_maximum() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let files = devgen::generate(&lib, &facts, "core/dsp-link").unwrap();
    let h = &files
        .iter()
        .find(|f| f.0 == "build/gen/dl-limits.h")
        .expect("a c-shared file")
        .1;
    // 2^17 words of 4 bytes: the offset field of a chunk; not a device's area (DN2 has 0x50000)
    assert!(h.contains("#define DL_AREA_MAX_BYTES 0x80000u"), "{h}");
    assert!(h.contains("#define DL_AREA_MAX_WORDS 0x20000u"), "{h}");
}

/// The modules of the rt021 flash images (`out/patch/rt021-dt2-1.17.plan.json`, `modules`), by id.
const RT021_MODULES: [&str; 7] = [
    "core/dsp-link",
    "core/dsp-voice",
    "core/dyndsp",
    "core/rows",
    "core/expansion",
    "core/health",
    "core/runtime",
];

#[test]
fn a_device_without_a_dsp_patches_the_runtime_menu_and_panel_modules_only() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    // the runtime, what it stands on, core/ui (the DIGIKIT menu is the system package's, not an image module), the panel modules and the peer image (the second CPU's section stored packed); no DSP, machine or effect module
    for dev in devdata::all()
        .iter()
        .filter(|d| !d.patch_sets.is_empty() && d.identity.capabilities.dsp == "none")
    {
        for (set, roots) in &dev.patch_sets {
            let (mut all, _) = lib.with_requires(roots).unwrap();
            all.sort();
            // a Syntakt's optional first flash (`device-peer`) is the peer image alone: no runtime, menu or panel module
            if set == "device-peer" {
                assert_eq!(all, ["core/peer-image"], "{} set {set}", dev.short);
                continue;
            }
            let mut want = vec![
                "core/expansion",
                "core/health",
                "core/input",
                "core/peer-image",
                "core/platform",
                "core/runtime",
                "core/ui",
            ];
            // a device that turns the eMMC write guard on holds core/card-guard in every set with the runtime
            if dev.switches.get("card.guard.enabled") == Some(&true) {
                want.insert(0, "core/card-guard");
            }
            assert_eq!(all, want, "{} set {set}", dev.short);
        }
    }
}

#[test]
fn the_patch_sets_are_the_rt021_flash_modules() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    // the sets of a device with a DSP are the DSP machine and effect modules below; a device without one (the runtime, menu and panel modules) is
    // checked by `a_device_without_a_dsp_patches_the_runtime_menu_and_panel_modules_only`
    for dev in devdata::all()
        .iter()
        .filter(|d| !d.patch_sets.is_empty() && d.identity.capabilities.dsp == "sharc")
    {
        // both sets are the flash modules and core/input
        for (set, extra) in [
            ("device", vec!["core/input"]),
            ("emulator", vec!["core/input"]),
        ] {
            let roots = dev.patch_set(set).unwrap();
            let (mut all, _) = lib.with_requires(roots).unwrap();
            all.sort();
            let mut want: Vec<&str> = RT021_MODULES.to_vec();
            want.extend(extra);
            // run-time effects (core/dynfx and the chain it reserves rows in): both devices have them in both sets (the Digitone II's emulator set since the engine bars only the
            // changed blocks: the hook costs x1.01 DSP CPU there, finding 55)
            want.extend(["core/dynfx", "core/fx"]);
            // core/runtime requires the platform adapter (its hooks and the exception hook install)
            want.push("core/platform");
            // the SETTINGS hook of the menu, the controls and the views of modules (the DIGIKIT menu in the system package is drawn by it)
            want.push("core/ui");
            // the sample pool: both Digitakt II sets have it; the Digitone II flashes without it until the SPI wire-size decision
            if dev.short == "dt2" || set == "emulator" {
                want.push("core/sample-pool");
            }
            // the flashed sets carry the fast-transfer adapters, inert until usb-fast asks: USB bulk on both devices, +Drive writes on the Digitakt II;
            // core/sdhc-large (large card commands for registered buffers) is in every flashed set
            if set == "device" {
                want.push("core/usb-bulk");
                want.push("core/usb-bulk-audio");
                if dev.short == "dt2" {
                    want.push("core/plusdrive-write");
                }
            }
            if set.starts_with("device") {
                want.push("core/sdhc-large");
            }
            want.sort();
            assert_eq!(all, want, "{} set {set}", dev.short);
        }
        // the Workbench's Custom boot screen toggle: the `device` modules and the optional black intro
        let (mut with, _) = lib
            .with_requires(dev.patch_set("device-introblank").unwrap())
            .unwrap();
        let (mut base, _) = lib.with_requires(dev.patch_set("device").unwrap()).unwrap();
        base.push("core/introblank".to_string());
        with.sort();
        base.sort();
        assert_eq!(with, base, "{} set device-introblank", dev.short);
        assert!(dev.patch_set("nope").is_err());
    }
}

#[test]
fn the_input_facts_are_the_device_files_panel() {
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let files = devgen::generate(&lib, &facts, "core/input").unwrap();
    let get = |p: &str| &files.iter().find(|f| f.0 == p).unwrap().1;
    // DT2 lists 49 = [6, 1] and 50 = [6, 0]; DN2 adds 51 to 54 = [6, 4 to 7]
    for (dev, wants) in [
        (
            "dt2",
            vec![
                "PANEL_EXC_N, 0x2 ",
                "PANEL_EXC_CODE_0, 0x31 ",
                "PANEL_EXC_BIT_0, 0x1 ",
                "PANEL_EXC_CODE_1, 0x32 ",
                "PANEL_EXC_BIT_1, 0x0 ",
            ],
        ),
        (
            "dn2",
            vec![
                "PANEL_EXC_N, 0x6 ",
                "PANEL_EXC_CODE_5, 0x36 ",
                "PANEL_EXC_BIT_5, 0x7 ",
            ],
        ),
    ] {
        let text = get(&format!("build/gen/input-{dev}.inc"));
        for want in [
            "INPUT_BACKEND, 0x1 ",
            "PANEL_LINEAR, 0x6 ",
            "PANEL_ENCODERS, 0x9 ",
            "PANEL_BUTTON_CHANNELS, 0x7 ",
        ]
        .into_iter()
        .chain(wants)
        {
            assert!(
                text.contains(&format!(".equ {want}")),
                "{dev}: {want}\n{text}"
            );
        }
    }
}

#[test]
fn an_unknown_backend_or_a_bad_panel_exception_is_refused() {
    let (dt2, dn2) = device_texts();
    for (what, from, to) in [
        (
            "backend",
            "backend = \"panel-uart\"",
            "backend = \"carrier-pigeon\"",
        ),
        ("exceptions", "49 = [6, 1]", "49 = [3, 1]"),
        ("exceptions", "49 = [6, 1]", "49 = [6, 9]"),
        // a channel at or above button_channels is outside the decoder's tables
        ("exceptions", "49 = [6, 1]", "49 = [7, 1]"),
        (
            "button_channels",
            "button_channels = 7",
            "button_channels = 5",
        ),
    ] {
        let bad = dt2.replace(from, to);
        assert_ne!(bad, dt2, "{what}: the test text did not change");
        let e = Facts::from_texts([bad.as_str(), dn2.as_str()])
            .unwrap_err()
            .0;
        assert!(e.contains(what), "{what}: {e}");
    }
}

#[test]
fn the_sample_pool_facts_are_checked() {
    let (dt2, dn2) = device_texts();
    // the committed files: DT2 borrows stock slots, DN2 owns a DDR range of the DSP
    Facts::from_texts([dt2.as_str(), dn2.as_str()]).unwrap();
    let on_dt2 = |from: &str, to: &str| (dt2.replace(from, to), dn2.clone());
    let on_dn2 = |from: &str, to: &str| (dt2.clone(), dn2.replace(from, to));
    for (what, (a, b)) in [
        (
            "a slot floor that is a multiple of 0x80 (the loader refuses those slots)",
            on_dt2("slot_floor = 0x3f0", "slot_floor = 0x380"),
        ),
        (
            "a slot floor above the last stock slot",
            on_dt2("slot_floor = 0x3f0", "slot_floor = 0x400"),
        ),
        (
            "a range with a base and no size",
            on_dn2("ddr_bytes  = 0x0ff80000", "ddr_bytes  = 0"),
        ),
        (
            "a range below the DSP's DDR",
            on_dn2("ddr_base   = 0x80531000", "ddr_base   = 0x40531000"),
        ),
        (
            "an unaligned range",
            on_dn2("ddr_base   = 0x80531000", "ddr_base   = 0x80531800"),
        ),
        (
            "a range past 4 GiB",
            on_dn2("ddr_bytes  = 0x0ff80000", "ddr_bytes  = 0x90000000"),
        ),
        (
            "R rings inside the pool range",
            on_dn2("ring_r_base = 0x904b1000", "ring_r_base = 0x90000000"),
        ),
        (
            "R rings below the stock writes",
            on_dn2("ring_r_base = 0x904b1000", "ring_r_base = 0x80530000"),
        ),
        (
            "a stock floor and an own range together",
            on_dn2("slot_floor = 0", "slot_floor = 0x3f0"),
        ),
    ] {
        assert!(a != dt2 || b != dn2, "{what}: the test text did not change");
        let e = Facts::from_texts([a.as_str(), b.as_str()]).unwrap_err().0;
        assert!(e.contains("sample_pool"), "{what}: {e}");
    }
}

#[test]
fn only_runtime_targets_enter_the_shared_constants() {
    // an identity-only file (no [digikit]) is skipped; a [digikit] without its board tables is an error
    let ident = "[device]\nname = \"X\"\nshort = \"x\"\n";
    assert!(Facts::from_texts([ident]).is_ok());
    let half = "[device]\nname = \"X\"\nshort = \"x\"\n[digikit]\nid = 9\nname = \"x\"\n";
    assert!(Facts::from_texts([half]).is_err());
}

/// The words of a device's `dkrt_dev` as generated for `core/runtime`.
fn device_object(facts: &Facts, dev: &str) -> Vec<u64> {
    let lib = Library::scan_repo_or_skip(&root()).expect("the repository's modules");
    let files = devgen::generate(&lib, facts, "core/runtime").unwrap();
    let text = &files
        .iter()
        .find(|f| f.0 == format!("build/gen/dkdv-{dev}.inc"))
        .unwrap()
        .1;
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(".long "))
        .map(|l| {
            let w = l.split_whitespace().next().unwrap();
            u64::from_str_radix(w.trim_start_matches("0x"), 16).unwrap()
        })
        .collect()
}

#[test]
fn the_logical_keys_are_numbered_with_the_trig_keys_in_a_row() {
    let (dt2, dn2) = device_texts();
    let facts = Facts::from_texts([dt2.as_str(), dn2.as_str()]).unwrap();
    let names = facts.key_names().unwrap();
    let number = |n: &str| names.iter().position(|x| x == n).unwrap() + 1;
    for n in 1..=16 {
        assert_eq!(number(&format!("TRIG{n}")), number("TRIG1") + n - 1);
    }
    let h = devgen::keys_h(&facts).unwrap();
    assert!(h.contains("#define DK_KEY_NONE 0\n"), "{h}");
    assert!(
        h.contains(&format!("#define DK_KEY_TRK {}\n", number("TRK"))),
        "{h}"
    );
    assert!(
        h.contains("#define DK_KEY_TRIG(n) ((dk_key)(DK_KEY_TRIG1 + (n) - 1))"),
        "{h}"
    );
    assert!(
        h.contains(&format!("#define DK_KEY_COUNT {} ", names.len())),
        "{h}"
    );
}

#[test]
fn the_key_map_gives_each_device_its_codes_and_a_missing_key_is_zero() {
    let (dt2, dn2) = device_texts();
    let facts = Facts::from_texts([dt2.as_str(), dn2.as_str()]).unwrap();
    let names = facts.key_names().unwrap();
    let number = |n: &str| names.iter().position(|x| x == n).unwrap() + 1;
    // the words of the object: 15 fields, `tracks`, then the key codes indexed by the key's number, then the four key flag bits
    let (tracks, keys) = (15, 16);
    let (d, n) = (device_object(&facts, "dt2"), device_object(&facts, "dn2"));
    let key = |o: &[u64], name: &str| o[keys + number(name)];
    for o in [&d, &n] {
        assert_eq!(o[tracks], 16);
        assert_eq!(o[keys], 0, "entry 0 is DK_KEY_NONE");
        assert_eq!(key(o, "TRK"), 16);
        assert_eq!(key(o, "YES"), 10);
        assert_eq!(key(o, "TRIG10"), 34);
        assert_eq!(key(o, "ENC3"), 43);
    }
    // code 2 is one logical key on both devices (the DN2 panel prints SYN)
    assert_eq!((key(&d, "SRC"), key(&n, "SRC")), (2, 2));
}

#[test]
fn the_profile_has_the_same_key_codes_as_the_device_object() {
    let (dt2, dn2) = device_texts();
    let facts = Facts::from_texts([dt2.as_str(), dn2.as_str()]).unwrap();
    let lib = Library::scan_repo_or_skip(&root()).expect("the repository's modules");
    let files = devgen::generate(&lib, &facts, "core/runtime").unwrap();
    let profiles = &files
        .iter()
        .find(|f| f.0.ends_with("profiles.rs"))
        .unwrap()
        .1;
    for dev in ["dt2", "dn2"] {
        let obj = device_object(&facts, dev);
        let from = profiles
            .find(&format!("pub const {}:", dev.to_uppercase()))
            .unwrap();
        let rest = &profiles[from..];
        let list = rest
            .split("key_codes: [")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        let codes: Vec<u64> = list
            .split(',')
            .filter_map(|c| c.trim().parse().ok())
            .collect();
        assert_eq!(codes.len(), 64, "{dev}");
        assert_eq!(codes[..], obj[obj.len() - 68..obj.len() - 4], "{dev}");
        let bits: Vec<u64> = rest
            .split("key_flags: KeyFlagBits {")
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap()
            .split(',')
            .filter_map(|f| f.split(':').nth(1)?.trim().parse().ok())
            .collect();
        assert_eq!(bits[..], obj[obj.len() - 4..], "{dev}: the key flag bits");
    }
}

#[test]
fn a_key_table_with_a_clash_or_a_bad_name_is_refused() {
    let (dt2, dn2) = device_texts();
    for (what, from, to, want) in [
        ("clash", "TRK = 16", "TRK = 17", "same code"),
        ("lower case", "TRK = 16", "trk = 16", "capital letters"),
        ("zero", "TRK = 16", "TRK = 0", "1 to 255"),
        ("too big", "TRK = 16", "TRK = 256", "1 to 255"),
    ] {
        let bad = dt2.replace(from, to);
        assert_ne!(bad, dt2, "{what}: the text did not change");
        let e = Facts::from_texts([bad.as_str(), dn2.as_str()])
            .unwrap_err()
            .0;
        assert!(e.contains(want), "{what}: {e}");
    }
    // a name that is not in `[keys] order` of sdk/sdk.toml
    let bad = dt2.replace("TRK = 16", "TRK = 16\nNOT_A_KEY = 100");
    let e = Facts::from_texts([bad.as_str(), dn2.as_str()])
        .unwrap()
        .key_names()
        .unwrap_err()
        .0;
    assert!(e.contains("NOT_A_KEY") && e.contains("[keys] order"), "{e}");
}

#[test]
fn the_logical_key_numbers_are_the_order_of_the_sdk_list_and_do_not_depend_on_the_device_files() {
    let (dt2, dn2) = device_texts();
    let all = Facts::from_texts([dt2.as_str(), dn2.as_str()])
        .unwrap()
        .key_names()
        .unwrap();
    let one = Facts::from_texts([dn2.as_str()])
        .unwrap()
        .key_names()
        .unwrap();
    assert_eq!(all, one);
    assert_eq!(all.first().map(String::as_str), Some("PAGE_TRIG"));
    assert_eq!(
        all.iter().position(|n| n == "TRK"),
        Some(15),
        "the number of a name is fixed by the list (append only)"
    );
}

#[test]
fn a_module_that_needs_device_tables_gets_a_device_that_has_them() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    // every device of the repository has the tables of the modules in its patch sets
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    patchkit::devgen::check_device_tables(&lib, &facts).unwrap();
    // core/ui declares `input.keys`: a device file without it is refused, naming the table, the set and the module
    let dt2 = std::fs::read_to_string(root().join("devices/digitakt-ii.toml")).unwrap();
    let start = dt2
        .find("[input.keys]")
        .expect("the device file has the table");
    let end = dt2[start..].find("\n\n").map_or(dt2.len(), |n| start + n);
    let without = format!("{}{}", &dt2[..start], &dt2[end..]);
    let facts = Facts::from_texts([without.as_str()]).unwrap();
    let e = patchkit::devgen::check_device_tables(&lib, &facts)
        .unwrap_err()
        .0;
    assert!(
        e.contains("needs [input.keys]") && e.contains("core/ui"),
        "{e}"
    );
    // the generator of the module alone refuses the same device, and a patch set that names a module the library lacks is an error
    let e = devgen::generate(&lib, &facts, "core/ui").unwrap_err().0;
    assert!(e.contains("needs [input.keys]"), "{e}");
    let bare = r#"[device]
short = "xx"
name = "Bare"
[patch_sets]
device = ["core/ui"]
"#;
    let facts = Facts::from_texts([bare]).unwrap();
    let e = patchkit::devgen::check_device_tables(&lib, &facts)
        .unwrap_err()
        .0;
    assert!(
        e.contains("xx: patch set device includes core/ui"),
        "a device file without [digikit] is checked too: {e}"
    );
    let ghost = bare.replace("core/ui", "core/no-such-module");
    let facts = Facts::from_texts([ghost.as_str()]).unwrap();
    let e = patchkit::devgen::check_device_tables(&lib, &facts)
        .unwrap_err()
        .0;
    assert!(e.contains("core/no-such-module"), "{e}");
}

/// The ui constants of a device, generated for `core/ui`.
fn ui_constants(lib: &Library, facts: &Facts, file: &str) -> String {
    devgen::generate(lib, facts, "core/ui")
        .unwrap()
        .into_iter()
        .find(|(p, _)| p.ends_with(file))
        .unwrap_or_else(|| panic!("{file}"))
        .1
}

/// core/ui's four keys come from `[input.keys]` by name: YES, LEVEL_PUSH (the second YES), NO and RIGHT, each device with its own ids.
#[test]
fn the_ui_keys_are_resolved_by_name_from_the_input_keys() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    for (file, yes, yes2, no, right) in [
        ("ui-dt2.inc", "0xa", "0x31", "0xc", "0xf"),
        ("ui-dn2.inc", "0xa", "0x31", "0xc", "0xf"),
        // the Syntakt's key indices are not the panel codes (finding 153)
        ("ui-st.inc", "0xc", "0x30", "0xd", "0x11"),
    ] {
        let inc = ui_constants(&lib, &facts, file);
        for (name, value) in [
            ("UI_KEY_YES", yes),
            ("UI_KEY_YES2", yes2),
            ("UI_KEY_NO", no),
            ("UI_KEY_RIGHT", right),
        ] {
            assert!(
                inc.contains(&format!(".equ {name}, {value} ")),
                "{file}: {name}\n{inc}"
            );
        }
    }
}

/// A device that names keys but lacks one of core/ui's four is refused at generate, naming the key.
#[test]
fn a_device_without_one_of_the_ui_keys_is_refused() {
    let st = std::fs::read_to_string(root().join("devices/syntakt.toml")).unwrap();
    for name in ["YES", "NO", "RIGHT", "LEVEL_PUSH"] {
        let line = st
            .lines()
            .find(|l| l.starts_with(&format!("{name} = ")))
            .unwrap();
        let bad = st.replace(&format!("{line}\n"), "");
        assert_ne!(bad, st);
        let e = Facts::from_texts([bad.as_str()])
            .err()
            .unwrap_or_else(|| panic!("{name}: refused"))
            .0;
        assert!(
            e.contains(&format!("has no {name}")) && e.contains("core/ui"),
            "{name}: {e}"
        );
    }
}

/// `[input.key_flags]` names the bits of a key event's flags word by meaning: it is generated for core/ui and the device object, range checked, distinct and required.
#[test]
fn the_key_flag_table_is_generated_and_checked() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let st = std::fs::read_to_string(root().join("devices/syntakt.toml")).unwrap();
    let facts = Facts::from_texts([st.as_str()]).unwrap();
    // core/ui reads the press and release bits; the device object has all four (after the key codes)
    let inc = ui_constants(&lib, &facts, "ui-st.inc");
    for (name, value) in [("KEY_FLAG_PRESS", "0x0"), ("KEY_FLAG_RELEASE", "0x4")] {
        assert!(
            inc.contains(&format!(".equ {name}, {value} ")),
            "{name}\n{inc}"
        );
    }
    let object = device_object(&facts, "st");
    assert_eq!(
        object[object.len() - 4..],
        [0, 1, 3, 4],
        "press, func, repeat, release"
    );
    let bad = st.replace("release = 4", "release = 32");
    let e = Facts::from_texts([bad.as_str()])
        .err()
        .expect("32 is refused")
        .0;
    assert!(e.contains("release = 32"), "{e}");
    let bad = st.replace("release = 4", "release = 3");
    let e = Facts::from_texts([bad.as_str()]).err().expect("a clash").0;
    assert!(e.contains("release and repeat are both bit 3"), "{e}");
    let start = st.find("[input.key_flags]").unwrap();
    let end = st[start..].find("\n\n").map_or(st.len(), |n| start + n);
    let without = format!("{}{}", &st[..start], &st[end..]);
    let e = Facts::from_texts([without.as_str()])
        .err()
        .expect("required")
        .0;
    assert!(e.contains("[input.key_flags]"), "{e}");
}

#[test]
fn a_module_is_checked_against_its_own_tables_in_any_library() {
    // `pack --manifest` loads a library without the core modules: the patch sets of the repository's devices cannot resolve in it
    let manifest = root().join("tests/fixtures/modules/c-demo/module.toml");
    let defines: Vec<String> = vec![];
    let only = vec!["test/c-demo".to_string()];
    let build = patchkit::module::Build {
        root: &root(),
        only: Some(&only),
        defines: &defines,
    };
    let lib =
        Library::from_manifests(build, &[manifest], true, &only).expect("the fixture library");
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    let need = vec!["input.keys".to_string()];
    // the repository's devices have the table, whatever the library can resolve
    devgen::check_module_tables(&lib, &facts, "test/c-demo", &need).unwrap();
    assert!(
        devgen::check_device_tables(&lib, &facts).is_err(),
        "the whole-library check reports the unresolved roots"
    );
    // a device without the table that lists the module as a root of a set that cannot resolve here is refused ...
    let bare = r#"[device]
short = "xx"
[patch_sets]
device = ["core/ui", "test/c-demo"]
"#;
    let facts = Facts::from_texts([bare]).unwrap();
    let e = devgen::check_module_tables(&lib, &facts, "test/c-demo", &need)
        .unwrap_err()
        .0;
    assert!(
        e.contains("xx: patch set device includes test/c-demo") && e.contains("input.keys"),
        "{e}"
    );
    // ... a module that needs nothing is not stopped by it, and a set that cannot resolve and does not list the module is skipped
    devgen::check_module_tables(&lib, &facts, "test/c-demo", &[]).unwrap();
    let other = bare.replace(", \"test/c-demo\"", "");
    let facts = Facts::from_texts([other.as_str()]).unwrap();
    devgen::check_module_tables(&lib, &facts, "test/c-demo", &need).unwrap();
}

/// The Syntakt file with `from` replaced by `to` (the test text must differ from the file).
fn syntakt_with(from: &str, to: &str) -> String {
    let st = std::fs::read_to_string(root().join("devices/syntakt.toml")).unwrap();
    assert!(st.contains(from), "the Syntakt file has {from:?}");
    st.replace(from, to)
}

#[test]
fn a_device_that_turns_the_card_guard_on_needs_it_in_every_set_with_the_runtime() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    // the repository's own sets are consistent (also checked by `patchkit gen`)
    let facts = Facts::from_dir(&root().join("devices")).unwrap();
    patchkit::devgen::check_device_tables(&lib, &facts).unwrap();
    // the `device` and `emulator` sets with the runtime and without core/card-guard are refused, naming both modules and the switch
    for set in ["device", "emulator"] {
        let line = std::fs::read_to_string(root().join("devices/syntakt.toml"))
            .unwrap()
            .lines()
            .find(|l| l.starts_with(&format!("{set} = [")))
            .unwrap()
            .to_string();
        let without = syntakt_with(&line, &line.replace("\"core/card-guard\", ", ""));
        let facts = Facts::from_texts([without.as_str()]).unwrap();
        let e = patchkit::devgen::check_device_tables(&lib, &facts)
            .unwrap_err()
            .0;
        assert!(
            e.contains(&format!(
                "st: patch set {set} includes core/runtime but not core/card-guard"
            )) && e.contains("card.guard.enabled"),
            "{set}: {e}"
        );
    }
    // a set without the runtime (device-peer) needs no guard
    let facts = Facts::from_texts([syntakt_with(
        "device-peer = [\"core/peer-image\"]",
        "device-peer = [\"core/peer-image\"]",
    )
    .as_str()])
    .unwrap();
    patchkit::devgen::check_device_tables(&lib, &facts).unwrap();
}

#[test]
fn the_card_guard_module_is_refused_where_the_switch_is_off() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let off = syntakt_with(
        "[card.guard]\nenabled = true",
        "[card.guard]\nenabled = false",
    );
    let facts = Facts::from_texts([off.as_str()]).unwrap();
    let e = patchkit::devgen::check_device_tables(&lib, &facts)
        .unwrap_err()
        .0;
    assert!(
        e.contains(
            "includes core/card-guard, but the device file does not turn [card.guard.enabled] on"
        ),
        "{e}"
    );
    // a device file without the table is refused by `needs_device` first
    let st = syntakt_with("\n[card.guard]\n", "\n[card.guard]\n");
    let (head, rest) = st.split_once("\n[card.guard]\n").unwrap();
    let tail = rest
        .split_once("# Board facts that only the emulator uses")
        .unwrap()
        .1;
    let none = format!("{head}\n# Board facts that only the emulator uses{tail}");
    let facts = Facts::from_texts([none.as_str()]).unwrap();
    let e = patchkit::devgen::check_device_tables(&lib, &facts)
        .unwrap_err()
        .0;
    assert!(e.contains("needs [card.guard]"), "{e}");
}

#[test]
fn the_resolved_selection_of_apply_is_checked_the_same_way() {
    use std::collections::BTreeMap;
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let on: BTreeMap<String, bool> = [("card.guard.enabled".to_string(), true)].into();
    let off: BTreeMap<String, bool> = BTreeMap::new();
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let check = |sw: &BTreeMap<String, bool>, v: &[&str]| {
        patchkit::devgen::check_needed_when(&lib, "st", sw, &ids(v), "the module selection")
    };
    check(&on, &["core/runtime", "core/card-guard"]).unwrap();
    check(&on, &["core/peer-image"]).unwrap();
    check(&off, &["core/runtime"]).unwrap();
    let e = check(&on, &["core/runtime", "core/ui"]).unwrap_err().0;
    assert!(
        e.contains("st: the module selection includes core/runtime but not core/card-guard"),
        "{e}"
    );
    let e = check(&off, &["core/runtime", "core/card-guard"])
        .unwrap_err()
        .0;
    assert!(
        e.contains("includes core/card-guard, but the device file does not turn"),
        "{e}"
    );
}
