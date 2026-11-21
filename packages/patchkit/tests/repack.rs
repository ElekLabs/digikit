//! `[[repack]]`: a module that unpacks a read-only section lets patchkit store it packed. Over the real Syntakt image and port (skipped when the image
//! is absent); the modules are synthetic, apart from the real `emulator` set.

use patchkit::apply::{self, Options};
use patchkit::firmware::Firmware;
use patchkit::module::{Library, Module};
use patchkit::port::Registry;
use std::path::PathBuf;

const ST: &str = "Syntakt_OS1.42.syx";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn image() -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware").join(ST)).ok()
}

fn registry() -> Registry {
    Registry::from_dir(&root().join("ports")).unwrap()
}

/// A module with no object and the given manifest body.
fn module(id: &str, body: &str) -> Module {
    let text = format!(
        "[module]\nid = \"{id}\"\nversion = \"1.0.0\"\nname = \"{id}\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"synthetic\"\n{body}\n"
    );
    Module::from_parts(&text, &|_p: &str| -> patchkit::Result<String> {
        unreachable!("no object")
    })
    .unwrap()
}

/// A `[[repack]]` of the second CPU's section with the layout of the real adapter, changed by `edit`.
fn claim(raw: &[u8], edit: impl Fn(&mut u64, &mut u64, &mut String, &mut String)) -> String {
    let fw = Firmware::parse(raw).unwrap();
    let data = &fw
        .section(fw.device.role_section("cf2").unwrap())
        .unwrap()
        .data;
    let (mut dest, mut end) = (0x4f84f2c6u64, 0x4f8af2c1u64);
    let (mut role, mut sha) = ("cf2".to_string(), patchkit::error::sha256_hex(data));
    edit(&mut dest, &mut end, &mut role, &mut sha);
    format!(
        "[[repack]]\nrole = \"{role}\"\ndest = {dest}\nstage_end = {end}\nlen = {}\nsha256 = \"{sha}\"\n",
        data.len()
    )
}

fn run(raw: &[u8], mods: Vec<Module>, cap: bool) -> patchkit::Result<apply::Applied> {
    let mut lib = Library::scan_repo_for(&root(), true, Some(&[])).unwrap();
    let ids: Vec<String> = mods.iter().map(|m| m.id().to_string()).collect();
    lib.modules.extend(mods);
    // no record: a set of synthetic modules without an object has nowhere to put it
    let opts = Options {
        flash_cap: cap,
        record: false,
    };
    apply::apply_with(raw, &registry(), &lib, &ids, opts)
}

fn expect_err(r: patchkit::Result<apply::Applied>, needle: &str) {
    match r {
        Ok(_) => panic!("expected an error containing {needle:?}"),
        Err(e) => assert!(e.0.contains(needle), "{:?} lacks {needle:?}", e.0),
    }
}

fn stored7(syx: &[u8]) -> Vec<u8> {
    let fw = Firmware::parse(syx).unwrap();
    fw.section(7).unwrap().stored.clone()
}

#[test]
fn section_7_is_stored_packed_only_when_a_module_claims_it() {
    let Some(raw) = image() else { return };
    let stock = Firmware::parse(&raw).unwrap();
    let stock7 = stock.section(7).unwrap();

    let none = run(&raw, vec![module("t/none", "")], false).unwrap();
    assert_eq!(
        stored7(&none.syx),
        stock7.stored,
        "no claim: the stock bytes"
    );

    let m = module("t/claim", &claim(&raw, |_, _, _, _| {}));
    let out = run(&raw, vec![m], false).unwrap();
    let got = Firmware::parse(&out.syx).unwrap();
    let s = got.section(7).unwrap();
    assert_eq!(s.data, stock7.data, "the decoded section is the stock one");
    assert!(
        s.stored.len() < stock7.stored.len() * 3 / 4,
        "{} bytes",
        s.stored.len()
    );
    // verify proves it against the stock image, and says how
    let report =
        patchkit::verify::verify_unrecorded(&out.syx, Some(&registry()), Some(&raw)).unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.contains("section 7 is stored packed"))
    );
}

#[test]
fn a_claim_that_patchkit_cannot_honour_is_refused() {
    let Some(raw) = image() else { return };
    // a section that the device file does not allow, and a role the device lacks
    for role in ["fpga", "main", "dsp"] {
        let m = module("t/bad", &claim(&raw, |_, _, r, _| *r = role.into()));
        expect_err(run(&raw, vec![m], false), "[[repack]] role");
    }
    // two modules for one section
    let a = module("t/a", &claim(&raw, |_, _, _, _| {}));
    let b = module("t/b", &claim(&raw, |_, _, _, _| {}));
    expect_err(run(&raw, vec![a, b], false), "already claimed");
    // the module checks the stock section it knows: another content is refused at build time
    let m = module(
        "t/hash",
        &claim(&raw, |_, _, _, sha| *sha = "00".repeat(32)),
    );
    expect_err(run(&raw, vec![m], false), "not the one the module unpacks");
}

#[test]
fn a_buffer_layout_whose_unpack_would_overwrite_unread_stream_bytes_is_refused() {
    let Some(raw) = image() else { return };
    // the stream staged at the start of the output: the first bytes written land on bytes not read yet
    let stored =
        patchkit::aplib::pack_section(&Firmware::parse(&raw).unwrap().section(7).unwrap().data)
            .len() as u64;
    let tight = module(
        "t/tight",
        &claim(&raw, |dest, end, _, _| *end = *dest + stored),
    );
    expect_err(run(&raw, vec![tight], false), "margin");
    // a buffer too small for the stream
    let small = module(
        "t/small",
        &claim(&raw, |dest, end, _, _| *end = *dest + stored - 1),
    );
    expect_err(run(&raw, vec![small], false), "does not fit");
    // the real layout keeps a margin above 0 (finding 151: 9,444 bytes)
    let ok = module("t/ok", &claim(&raw, |_, _, _, _| {}));
    run(&raw, vec![ok], false).unwrap();
}

#[test]
fn the_syntakt_emulator_set_fits_the_flash_container_with_section_7_stored_packed() {
    let Some(raw) = image() else { return };
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let dev = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (ids, _) = lib.with_requires(&dev.patch_sets["emulator"]).unwrap();
    // with the flash cap: the set is refused when the container is over the limit
    let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
    let report = patchkit::verify::verify(&out.syx, Some(&reg), Some(&raw)).unwrap();
    assert!(report.container_bytes <= dev.container_max);
    let stock = Firmware::parse(&raw).unwrap();
    let got = Firmware::parse(&out.syx).unwrap();
    assert!(got.section(7).unwrap().stored.len() < stock.section(7).unwrap().stored.len());
    assert_eq!(got.section(7).unwrap().data, stock.section(7).unwrap().data);
}

/// The Rust margin equals the reference `tools.firmware.aplibpack.inplace_margin` on the real stream (9,444 bytes, finding 151).
#[test]
fn the_in_place_margin_of_the_real_section_7_stream_matches_the_python_reference() {
    let Some(raw) = image() else { return };
    let data = Firmware::parse(&raw)
        .unwrap()
        .section(7)
        .unwrap()
        .data
        .clone();
    let stream = patchkit::aplib::pack_section(&data);
    assert_eq!(stream.len(), 264_194);
    let stage = 0x4f8af2c1 - stream.len() as u64;
    assert_eq!(
        patchkit::aplib::inplace_margin(&stream, 0x4f84f2c6, stage).unwrap(),
        Some(9_444)
    );
}

#[test]
fn verify_accepts_a_packed_section_only_when_a_module_of_the_record_claims_it() {
    let Some(raw) = image() else { return };
    let Some(mut lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let dev = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (ids, _) = lib.with_requires(&dev.patch_sets["emulator"]).unwrap();
    let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
    // the claiming module is in the record: the claim and the layout hold
    let report =
        patchkit::verify::verify_with_library(&out.syx, Some(&reg), Some(&raw), &lib).unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.contains("its module declares the same"))
    );
    // the same image, with a library where no module claims the role: refused
    for m in lib.modules.iter_mut() {
        m.manifest.repack.clear();
    }
    let e = patchkit::verify::verify_with_library(&out.syx, Some(&reg), Some(&raw), &lib)
        .unwrap_err()
        .0;
    assert!(e.contains("does not declare the repack"), "{e}");
    // a module whose declared layout is not the record's is refused too
    let mut tight = Library::scan_repo_or_skip(&root()).unwrap();
    for m in tight.modules.iter_mut() {
        for r in m.manifest.repack.iter_mut() {
            r.stage_end = r.dest + 1000;
        }
    }
    let e = patchkit::verify::verify_with_library(&out.syx, Some(&reg), Some(&raw), &tight)
        .unwrap_err()
        .0;
    assert!(e.contains("does not declare the repack"), "{e}");
}

#[test]
fn a_repack_declaration_the_record_cannot_hold_is_refused_when_the_manifest_is_read() {
    let body = |extra: &str| {
        format!(
            "[module]\nid = \"t/r\"\nversion = \"1\"\nname = \"r\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"d\"\nstatus = \"experimental\"\nsummary = \"s\"\n[[repack]]\nrole = \"cf2\"\ndest = 16\nstage_end = 32\nlen = 4\nsha256 = \"{}\"\n{extra}",
            "ab".repeat(32)
        )
    };
    let read = |_: &str| -> patchkit::Result<String> { unreachable!() };
    assert!(Module::from_parts(&body(""), &read).is_ok());
    for bad in [
        body("").replace(&"ab".repeat(32), "zz"),
        body("").replace("dest = 16", "dest = 4294967296"),
        body("").replace("stage_end = 32", "stage_end = 8"),
        body("").replace("role = \"cf2\"", "role = \"\""),
    ] {
        assert!(Module::from_parts(&bad, &read).is_err(), "{bad}");
    }
}

#[test]
fn verify_without_a_library_needs_the_repack_entry_of_the_patch_record() {
    let Some(raw) = image() else { return };
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let dev = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (ids, _) = lib.with_requires(&dev.patch_sets["emulator"]).unwrap();
    let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
    // flash, preflight and the wasm callers have no library: the record's entry is what they check
    let report = patchkit::verify::verify(&out.syx, Some(&reg), Some(&raw)).unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.contains("the record is consistent with it"))
    );
    let rec = report.record.unwrap();
    assert_eq!(rec.repacks.len(), 1);
    assert_eq!(rec.repacks[0].role, "cf2");
    assert!(rec.modules.contains(&rec.repacks[0].module));

    // the same image with the entry block of its record cut off (the magic changed): a packed section that no module is known to unpack
    let mut fw = Firmware::parse(&out.syx).unwrap();
    let mut main = fw.main().unwrap().data.clone();
    let at = main.windows(4).position(|w| w == b"RPK1").expect("RPK1");
    main[at + 3] = b'0';
    // stored the way the guard needs it (the plain packer would leave a negative bootstrap margin)
    let (dest, stage, floor) = fw.device.main_unpack();
    let made = patchkit::aplib::pack_section_in_place(&main, dest, stage, floor as i64).unwrap();
    fw.replace_main_stored(main, made.stream).unwrap();
    let cut = fw.build(None).unwrap();
    let e = patchkit::verify::verify(&cut, Some(&reg), Some(&raw))
        .unwrap_err()
        .0;
    assert!(e.contains("no repack entry"), "{e}");
}

#[test]
fn the_step_a_set_carries_the_peer_image_and_its_claim_but_no_runtime() {
    let Some(raw) = image() else { return };
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let dev = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    // the optional first flash (`device-peer`): the peer image and what it needs to be loaded, no runtime, with its repack claim in the record
    let (ids, _) = lib.with_requires(&dev.patch_sets["device-peer"]).unwrap();
    let a = apply::apply(&raw, &reg, &lib, &ids).unwrap();
    let rec = patchkit::verify::verify(&a.syx, Some(&reg), Some(&raw))
        .unwrap()
        .record
        .unwrap();
    let names: Vec<&str> = rec
        .modules
        .iter()
        .map(|m| m.split('@').next().unwrap())
        .collect();
    assert_eq!(names, ["core/expansion", "core/peer-image"]);
    assert_eq!(rec.repacks.len(), 1);
}

#[test]
fn the_syntakt_device_set_keeps_the_bootstrap_margin_and_fits_the_container() {
    let Some(raw) = image() else { return };
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let dev = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (ids, _) = lib.with_requires(&dev.patch_sets["device"]).unwrap();
    // apply refuses an image below the floor and a container over the cap, so success is the proof; the numbers are read back
    let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
    let fw = Firmware::parse(&out.syx).unwrap();
    let (dest, stage, floor) = fw.device.main_unpack();
    let main = fw.main().unwrap();
    let m = patchkit::aplib::inplace_margin(&main.stored, dest, stage)
        .unwrap()
        .expect("the unpack reaches the stage");
    assert!(m >= floor as i64, "margin {m}");
    let report = patchkit::verify::verify(&out.syx, Some(&reg), Some(&raw)).unwrap();
    assert!(report.container_bytes <= dev.container_max);
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.starts_with("bootstrap margin "))
    );
    // the stock image has margin too (finding 152: +96,356 bytes)
    let stock = Firmware::parse(&raw).unwrap();
    let sm = patchkit::aplib::inplace_margin(&stock.main().unwrap().stored, dest, stage).unwrap();
    assert_eq!(sm, Some(96_356));
}

#[test]
fn the_digitakt_and_digitone_device_images_have_no_literal_tail() {
    for (name, short) in [
        ("Digitakt_II_OS1.17.syx", "dt2"),
        ("Digitone_II_OS1.12.syx", "dn2"),
    ] {
        let Ok(raw) = std::fs::read(root().join("firmware").join(name)) else {
            continue;
        };
        let Some(lib) = Library::scan_repo_or_skip(&root()) else {
            return;
        };
        let reg = registry();
        let dev = patchkit::devdata::all()
            .iter()
            .find(|d| d.short == short)
            .unwrap();
        let (ids, _) = lib.with_requires(&dev.patch_sets["device"]).unwrap();
        let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
        let fw = Firmware::parse(&out.syx).unwrap();
        let main = fw.main().unwrap();
        // the plain packer's stream: the images of these devices are what they were before the margin guard
        assert_eq!(
            main.stored,
            patchkit::aplib::pack_section(&main.data),
            "{short}"
        );
    }
}

/// The key ids of the Syntakt's key events that core/ui compares (`[input.keys]`) are the entries of the firmware's decoder table at 0x401d7a48 for the
/// wire positions of YES, NO, RIGHT and the second YES key (panel codes 39, 28, 31 and 49: wire position = code - 1 = channel * 8 + bit; finding 153).
#[test]
fn the_syntakt_input_keys_are_the_decoder_table_entries() {
    let Some(raw) = image() else { return };
    let fw = Firmware::parse(&raw).unwrap();
    let main = fw.main().unwrap();
    let table = |wire: usize| -> u64 {
        let at = (0x401d7a48 - main.dest as usize) + 4 * wire;
        u32::from_be_bytes(main.data[at..at + 4].try_into().unwrap()) as u64
    };
    let text = std::fs::read_to_string(root().join("devices/syntakt.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    let keys = &doc["input"]["keys"];
    let get = |k: &str| keys[k].as_integer().unwrap() as u64;
    assert_eq!(get("YES"), table(39 - 1));
    assert_eq!(get("NO"), table(28 - 1));
    assert_eq!(get("RIGHT"), table(31 - 1));
    // the second YES key is the LEVEL push, panel code 49 (channel 6, bit 0: wire position 48)
    assert_eq!(get("LEVEL_PUSH"), table(49 - 1));
}
