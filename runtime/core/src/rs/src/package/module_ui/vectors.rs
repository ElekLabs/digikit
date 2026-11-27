//! The shared vectors of the Workbench settings descriptor (`runtime/core/vectors/workbench-vectors.json`), read for the tests.

use serde_json::Value;
use std::string::String;
use std::vec::Vec;

pub(super) struct Descriptor {
    pub(super) name: String,
    pub(super) module: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct StoredValue {
    pub(super) descriptor: String,
    pub(super) setting: String,
    pub(super) bytes: Vec<u8>,
    pub(super) valid: bool,
    pub(super) rule: String,
}

fn read() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../vectors/workbench-vectors.json"
    ))
    .unwrap()
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap().to_string()
}

pub(super) fn descriptors() -> Vec<Descriptor> {
    read()["descriptors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| Descriptor {
            name: text(d, "name"),
            module: text(d, "module"),
            bytes: hex(&text(d, "bytes_hex")),
        })
        .collect()
}

pub(super) fn values() -> Vec<StoredValue> {
    read()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| StoredValue {
            descriptor: text(v, "descriptor"),
            setting: text(v, "setting"),
            bytes: hex(&text(v, "value_hex")),
            valid: v["valid"].as_bool().unwrap(),
            rule: text(v, "rule"),
        })
        .collect()
}

pub(super) fn hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}
