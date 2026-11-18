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
    Registry::from_dir(&root().join("devices/ports")).unwrap()
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
    let report = patchkit::verify::verify(&out.syx, Some(&registry()), Some(&raw)).unwrap();
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
    // the real layout keeps a margin above 0 (finding 150: 9,444 bytes)
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

/// The adapter's C code and its `[[repack]]` name the same buffer, length and hash: patchkit checks the stream against the manifest, the code
/// checks the unpacked bytes against its own constants.
#[test]
fn the_adapter_source_names_the_layout_of_its_manifest() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = lib.get("core/peer-image").expect("core/peer-image");
    let r = &m.manifest.repack[0];
    let src =
        std::fs::read_to_string(root().join("modules/core/adapters/peer-image/src/cf/peer.c"))
            .unwrap();
    let define = |name: &str| -> u64 {
        let line = src
            .lines()
            .find_map(|l| l.strip_prefix(&format!("#define {name} ")))
            .unwrap_or_else(|| panic!("no #define {name}"));
        let v = line
            .split_whitespace()
            .next()
            .unwrap()
            .trim_end_matches('u');
        match v.strip_prefix("0x") {
            Some(h) => u64::from_str_radix(h, 16).unwrap(),
            None => v.parse().unwrap(),
        }
    };
    assert_eq!(define("PEER_DEST"), r.dest);
    assert_eq!(define("PEER_STAGE_END"), r.stage_end);
    assert_eq!(define("PEER_LEN"), r.len);
    // the thread stores the length 4 bytes below the payload, behind the 0xAA byte
    assert_eq!(define("PEER_LEN_WORD"), r.dest - 4);
    let table = src
        .split("PEER_SHA256[32] = {")
        .nth(1)
        .and_then(|s| s.split("};").next())
        .unwrap();
    let hex: String = table
        .split(',')
        .filter_map(|b| b.trim().strip_prefix("0x"))
        .collect();
    assert_eq!(hex, r.sha256);
}

/// The Rust margin equals the reference `tools.firmware.aplibpack.inplace_margin` on the real stream (9,444 bytes, finding 150).
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
