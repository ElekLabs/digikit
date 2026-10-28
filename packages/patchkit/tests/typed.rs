//! Typed module sections (`[[machine]]`, `[[param]]`, `[[page]]`, `[[voice]]`) over the real device models, ports and
//! stock images (the image tests are skipped when the images are absent).
//!
//! The pins are the SHA-256 of every section except 7 of the images that the hand-written `[[entry]]` rows of
//! `test/patched-pluck` (then `digikit/pluck`) and `test/xslice` produced before they were re-expressed in the typed form (commit 8606b7f);
//! the typed form must lower to the same rows, so those sections must stay byte-identical on every port the modules
//! support. Section 7 (the SHARC DSP stream) is left out: it moves with every DSP edit to pluck or core/dsp-voice, which
//! the conformance, PCM and scenario tests guard (the whole-image pins moved seven times with section 7 only).

use patchkit::apply;
use patchkit::error::sha256_hex;
use patchkit::firmware::Firmware;
use patchkit::modgen;
use patchkit::module::Library;
use patchkit::port::Registry;
use patchkit::typed::Models;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn library() -> Library {
    Library::scan_repo(&root()).unwrap()
}

/// SHA-256 over the id, length and data of every section except 7 (the SHARC DSP stream), in container order.
fn digest_without_dsp(syx: &[u8]) -> String {
    let fw = Firmware::parse(syx).unwrap();
    let mut buf = Vec::new();
    for s in fw.sections.iter().filter(|s| s.id != 7) {
        buf.extend_from_slice(&s.id.to_be_bytes());
        buf.extend_from_slice(&(s.data.len() as u64).to_be_bytes());
        buf.extend_from_slice(&s.data);
    }
    sha256_hex(&buf)
}

/// (stock image, modules, digest_without_dsp of the patched image); moved 2026-10-20 with the id renames (P3c-3a) and 2026-10-21 with digikit/pluck -> test/patched-pluck (P3e-1): patch record only; 2026-10-26 with core/params' formatter word (a params row is 32 bytes) and its param_edits registry
const PINS: &[(&str, &str, &str)] = &[
    (
        "Digitakt_II_OS1.17.syx",
        "test/patched-pluck",
        "b01548f45dea574150b044cc281aabcfb63e07c104e90f664a2ca5916475ad9a",
    ),
    (
        "Digitakt_II_OS1.17.syx",
        "test/xslice",
        "b2e056f9bd7bd9d007546c3815796306560ba8f09c6c1084395fc5f3eb87d8a6",
    ),
    (
        "Digitakt_II_OS1.17.syx",
        "test/patched-pluck,test/xslice",
        "9b3144e28575f2c78613add357af74b77ed137528f03cf30d3fb114f3685f423",
    ),
    (
        "Digitone_II_OS1.12.syx",
        "test/patched-pluck",
        "4551666bf28ff4c393b20da71984267bc66db1c05dc5f9a41cf23e6c1e1a6787",
    ),
];

#[test]
fn typed_form_patches_to_the_images_of_the_hand_written_rows() {
    let lib = library();
    let reg = Registry::from_dir(&root().join("devices/ports")).unwrap();
    let mut ran = 0;
    for (file, mods, want) in PINS {
        let Ok(raw) = std::fs::read(root().join("firmware").join(file)) else {
            continue;
        };
        let ids: Vec<String> = mods.split(',').map(String::from).collect();
        let (ids, _) = lib.with_requires(&ids).unwrap();
        let out = apply::apply(&raw, &reg, &lib, &ids).unwrap();
        assert_eq!(
            &digest_without_dsp(&out.syx),
            want,
            "{file} + {mods}: a section other than 7 changed; if intended, set the pin to the left value"
        );
        ran += 1;
    }
    eprintln!("typed oracle: {ran} of {} pins ran", PINS.len());
}

#[test]
fn xslice_stays_off_the_digitone() {
    let lib = library();
    let reg = Registry::from_dir(&root().join("devices/ports")).unwrap();
    let Ok(raw) = std::fs::read(root().join("firmware/Digitone_II_OS1.12.syx")) else {
        return;
    };
    let ids = lib.with_requires(&["test/xslice".to_string()]).unwrap().0;
    let Err(e) = apply::apply(&raw, &reg, &lib, &ids) else {
        panic!("xslice applied to a Digitone II");
    };
    assert!(e.0.contains("not compatible"), "{e}");
}

/// The committed `build/gen/` files equal fresh ones (`patchkit gen` rewrites them).
#[test]
fn generated_files_are_current() {
    let lib = library();
    let models = Models::from_dir(&root().join("devices")).unwrap();
    let mut n = 0;
    for m in &lib.modules {
        for (rel, text) in modgen::generate(&lib, &models, m.id()).unwrap() {
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
    assert!(n >= 7, "pluck and xslice generate files, got {n}");
}

/// What pluck's DSP sources hard-code must be what the device models say (finding 04: frame words of the machine
/// slots; pluck.s: `0x6d + 48 t`, offsets 1, 2, 6, 7; pluck_dn2.s: `0x6d + 0x49 v`, offsets 0 .. 3).
#[test]
fn device_models_match_the_hard_coded_frame_layout_of_pluck() {
    let lib = library();
    let models = Models::from_dir(&root().join("devices")).unwrap();
    let files = modgen::generate(&lib, &models, "test/patched-pluck").unwrap();
    let g: serde_json::Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.0.ends_with("gallery.json"))
            .unwrap()
            .1,
    )
    .unwrap();
    let dt2 = &g["machines"][0]["devices"]["dt2"];
    assert_eq!(dt2["type"], 9);
    assert_eq!(dt2["knobs"][1]["param"], "pluck_decay");
    let inc = |dev: &str| {
        files
            .iter()
            .find(|f| f.0.ends_with(&format!("{dev}.inc")))
            .unwrap()
            .1
            .clone()
    };
    let (dt2, dn2) = (inc("dt2"), inc("dn2"));
    for want in [
        "#define FRAME_BASE 109 ",
        "#define FRAME_STRIDE 48 ",
        "#define PLUCK_DECAY_FRAME_OFF 1 ",
        "#define PLUCK_PICK_FRAME_OFF 2 ",
        "#define PLUCK_BRIGHT_FRAME_OFF 6 ",
        "#define PLUCK_COLOR_FRAME_OFF 7 ",
        "#define PLUCK_DECAY_MIRROR 26 ",
        "#define PLUCK_COLOR_MIRROR 32 ",
    ] {
        assert!(dt2.contains(want), "dt2: {want}");
    }
    for want in [
        "#define FRAME_STRIDE 73 ",
        "#define PLUCK_DECAY_FRAME_OFF 0 ",
        "#define PLUCK_COLOR_FRAME_OFF 3 ",
        "#define PLUCK_DECAY_SLOT 25 ",
        "#define PLUCK_COLOR_SLOT 28 ",
    ] {
        assert!(dn2.contains(want), "dn2: {want}");
    }
}

/// The bundle path of the browser: the same lowering from texts (ports, manifests, objects, device files).
#[test]
fn bundle_lowering_matches_the_scan() {
    let mut texts = vec![];
    for f in ["digitakt-ii.toml", "digitone-ii.toml"] {
        texts.push(std::fs::read_to_string(root().join("devices").join(f)).unwrap());
    }
    let models = Models::from_texts(texts.iter().map(|s| s.as_str())).unwrap();
    let dir = root().join("tests/fixtures/modules/patched-pluck");
    let text = std::fs::read_to_string(dir.join("module.toml")).unwrap();
    let read =
        |p: &str| std::fs::read_to_string(dir.join(p)).map_err(|e| patchkit::Error(e.to_string()));
    let m = patchkit::module::Module::from_parts(&text, &read).unwrap();
    let mut lib = Library { modules: vec![m] };
    lib.lower(&models).unwrap();
    let scanned = library();
    let a: Vec<_> = lib
        .get("test/patched-pluck")
        .unwrap()
        .manifest
        .entry
        .iter()
        .map(|e| (&e.registry, &e.id, &e.devices, &e.fields))
        .collect();
    let b: Vec<_> = scanned
        .get("test/patched-pluck")
        .unwrap()
        .manifest
        .entry
        .iter()
        .map(|e| (&e.registry, &e.id, &e.devices, &e.fields))
        .collect();
    assert_eq!(a, b);
    assert_eq!(
        a.len(),
        1 + 1 + 4 + 1 + 1 + 4 + 1,
        "dt2: voice, machine, 4 params; dn2: voice, machine, 4 params, page"
    );
}
