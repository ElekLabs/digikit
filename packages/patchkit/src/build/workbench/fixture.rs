//! Test data shared by the tests of this folder: the shared vectors.

use serde_json::Value;

/// The declaration of the vector `name` of `runtime/core/vectors/workbench-vectors.json`: (module id, module name, toml, bytes).
pub(super) struct VectorDescriptor {
    pub(super) name: String,
    pub(super) module: String,
    pub(super) module_name: String,
    pub(super) toml: String,
    pub(super) bytes: Vec<u8>,
    /// The `schema` constant of the TypeScript file patchkit generates for the declaration, as data.
    pub(super) expected_schema: Value,
}

/// A stored value of one setting and whether it is valid.
pub(super) struct VectorValue {
    pub(super) descriptor: String,
    pub(super) setting: String,
    pub(super) value: Vec<u8>,
    pub(super) valid: bool,
    pub(super) rule: String,
}

fn path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/core/vectors/workbench-vectors.json")
}

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap()
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap().to_string()
}

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

pub(super) fn vector_descriptors() -> Vec<VectorDescriptor> {
    load()["descriptors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| VectorDescriptor {
            name: text(d, "name"),
            module: text(d, "module"),
            module_name: text(d, "module_name"),
            toml: text(d, "toml"),
            bytes: unhex(&text(d, "bytes_hex")),
            expected_schema: d["expected_schema"].clone(),
        })
        .collect()
}

pub(super) fn vector_values() -> Vec<VectorValue> {
    load()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| VectorValue {
            descriptor: text(v, "descriptor"),
            setting: text(v, "setting"),
            value: unhex(&text(v, "value_hex")),
            valid: v["valid"].as_bool().unwrap(),
            rule: text(v, "rule"),
        })
        .collect()
}

/// Rewrites the `descriptors` section of the vectors file with `expected_schema` set to `expected(case)` for each case, in the layout
/// of the file (one space per level, fields in the order name, module, module_name, toml, bytes_hex, expected_schema). The rest of
/// the file stays byte for byte. Run the vectors test with `UPDATE_VECTORS=1` to call it.
pub(super) fn write_expected_schemas(expected: impl Fn(&VectorDescriptor) -> Value) {
    let file = std::fs::read_to_string(path()).unwrap();
    let start = file.find("\n \"descriptors\": [\n").unwrap() + 1;
    let end = file.find("\n \"values\": [\n").unwrap() + 1;
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let quote = |v: &str| serde_json::to_string(v).unwrap();
    let cases: Vec<String> = vector_descriptors()
        .iter()
        .map(|d| {
            let schema = serde_json::to_string_pretty(&expected(d)).unwrap();
            // Two-space indent of to_string_pretty becomes one space; the object sits at three.
            let schema = schema
                .lines()
                .map(|l| {
                    let lead = l.len() - l.trim_start().len();
                    format!("{}{}", " ".repeat(lead / 2), l.trim_start())
                })
                .collect::<Vec<_>>()
                .join("\n   ");
            format!(
                "  {{\n   \"name\": {},\n   \"module\": {},\n   \"module_name\": {},\n   \"toml\": {},\n   \"bytes_hex\": \"{}\",\n   \"expected_schema\": {}\n  }}",
                quote(&d.name),
                quote(&d.module),
                quote(&d.module_name),
                quote(&d.toml),
                hex(&d.bytes),
                schema
            )
        })
        .collect();
    let section = format!(" \"descriptors\": [\n{}\n ],\n", cases.join(",\n"));
    let text = format!("{}{}{}", &file[..start], section, &file[end..]);
    std::fs::write(path(), text).unwrap();
}
