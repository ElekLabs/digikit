//! Test data shared by the tests of this folder: a declaration with a text and a file-name setting, and the shared vectors.

use serde_json::Value;

pub(super) const DECLARATION: &str = r#"
[workbench]
version = 1
component = "workbench/Panel.tsx"

[[setting]]
id = "boot"
label = "État"
type = "text"
max_bytes = 16

[[setting]]
id = "active"
label = "Animation"
type = "file-name"
max_bytes = 47
apply = "next-restart"
extension = "dkbs"

[[operation]]
id = "readAtInit"
output = "optional-text"
"#;

/// The declaration of the vector `name` of `runtime/core/vectors/workbench-vectors.json`: (module id, module name, toml, bytes).
pub(super) struct VectorDescriptor {
    pub(super) name: String,
    pub(super) module: String,
    pub(super) module_name: String,
    pub(super) toml: String,
    pub(super) bytes: Vec<u8>,
}

/// A stored value of one setting and whether it is valid.
pub(super) struct VectorValue {
    pub(super) descriptor: String,
    pub(super) setting: String,
    pub(super) value: Vec<u8>,
    pub(super) valid: bool,
    pub(super) rule: String,
}

fn load() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/core/vectors/workbench-vectors.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap().to_string()
}

pub(super) fn unhex(hex: &str) -> Vec<u8> {
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
