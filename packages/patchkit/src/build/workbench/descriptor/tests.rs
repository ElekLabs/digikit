use super::*;
use crate::build::workbench::fixture::{DECLARATION, vector_descriptors};
use std::path::Path;

fn encode(source: &str, name: &str) -> Result<Vec<u8>> {
    Schema::parse(source, "test/module", name).map(|s| s.descriptor().to_vec())
}

#[test]
fn the_device_descriptor_contains_the_display_name_and_public_fields() {
    assert_eq!(
        encode(DECLARATION, "Démarrage").unwrap(),
        b"\x01\x0aD\xc3\xa9marrage\x02\x04boot\x05\xc3\x89tat\x10\x00\x00\
          \x06active\x09Animation\x2f\x01\x01\x04dkbs\x04None"
    );
}

#[test]
fn the_shared_vectors_give_the_descriptor_bytes() {
    for d in vector_descriptors() {
        let bytes = encode_for(&d.toml, &d.module, &d.module_name);
        assert_eq!(bytes, d.bytes, "{}", d.name);
    }
}

fn encode_for(source: &str, module: &str, name: &str) -> Vec<u8> {
    Schema::parse(source, module, name)
        .unwrap()
        .descriptor()
        .to_vec()
}

#[test]
fn each_new_type_writes_its_kind_and_body() {
    let source = |rest: &str| {
        format!("[workbench]\nversion = 1\n[[setting]]\nid = \"x\"\nlabel = \"X\"\n{rest}")
    };
    // integer: max 4, kind 2, apply 0, then min, max, step, decimals, default, unit
    assert_eq!(
        encode(
            &source(
                "type = \"integer\"\nmin = -1\nmax = 1\ndecimals = 2\nunit = \"%\"\ndefault = 1"
            ),
            "N"
        )
        .unwrap(),
        b"\x02\x01N\x01\x01x\x01X\x04\x02\x00\
          \xff\xff\xff\xff\x00\x00\x00\x01\x00\x00\x00\x01\x02\x00\x00\x00\x01\x01%"
    );
    assert_eq!(
        encode(
            &source("type = \"boolean\"\ndefault = true\napply = \"next-restart\""),
            "N"
        )
        .unwrap(),
        b"\x02\x01N\x01\x01x\x01X\x01\x03\x01\x01"
    );
    assert_eq!(
        encode(
            &source("type = \"choice\"\nchoices = [\"A\", \"Bc\"]\ndefault = \"Bc\""),
            "N"
        )
        .unwrap(),
        b"\x02\x01N\x01\x01x\x01X\x01\x04\x00\x02\x01\x01A\x02Bc"
    );
}

#[test]
fn a_version_1_module_encodes_as_before() {
    // the bytes the version 1 encoder made before the typed settings (taken from that encoder)
    for (dir, id, name, hex) in [(
        "modules/tool/tools/bootscreen",
        "tools/bootscreen",
        "Boot screen",
        "010b426f6f742073637265656e010661637469766509416e696d6174696f6e2f010104646b62730553746f636b",
    )] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let schema = Schema::read(&root.join(dir), id, Some(name))
            .unwrap()
            .unwrap();
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(schema.descriptor(), expected, "{dir}");
    }
}

#[test]
fn the_descriptor_allows_exactly_512_encoded_bytes() {
    let source = DECLARATION.replace("État", &"x".repeat(220));
    assert_eq!(encode(&source, &"x".repeat(250)).unwrap().len(), 512);
    assert!(
        encode(&source, &"x".repeat(251))
            .unwrap_err()
            .0
            .contains("exceeds 512")
    );
}

#[test]
fn a_descriptor_of_a_new_type_is_cut_at_512_bytes_to_the_byte() {
    // one choice setting whose labels fill the descriptor: the name pads it to the limit
    let labels: Vec<String> = (0..20)
        .map(|i| format!("\"label-{i:02}-xxxxxxxxxx\""))
        .collect();
    let source = format!(
        "[workbench]\nversion = 1\n[[setting]]\nid = \"m\"\nlabel = \"M\"\ntype = \"choice\"\nchoices = [{}]\ndefault = \"label-00-xxxxxxxxxx\"\n",
        labels.join(", ")
    );
    let base = encode(&source, "N").unwrap().len();
    assert!(base < 512);
    let at_512 = encode(&source, &"n".repeat(1 + 512 - base)).unwrap();
    assert_eq!(at_512.len(), 512);
    assert!(
        encode(&source, &"n".repeat(2 + 512 - base))
            .unwrap_err()
            .0
            .contains("exceeds 512")
    );
}
