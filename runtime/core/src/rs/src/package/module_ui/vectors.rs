//! The shared vectors of the Workbench settings descriptor (`runtime/core/vectors/workbench-vectors.json`), read for the tests.

use std::string::String;
use std::vec::Vec;

pub(super) struct Descriptor {
    pub(super) name: String,
    pub(super) module: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct Value {
    pub(super) descriptor: String,
    pub(super) setting: String,
    pub(super) bytes: Vec<u8>,
    pub(super) valid: bool,
    pub(super) rule: String,
}

pub(super) fn descriptors() -> Vec<Descriptor> {
    let all = read();
    all.field("descriptors")
        .items()
        .iter()
        .map(|d| Descriptor {
            name: d.field("name").text(),
            module: d.field("module").text(),
            bytes: hex(&d.field("bytes_hex").text()),
        })
        .collect()
}

pub(super) fn values() -> Vec<Value> {
    let all = read();
    all.field("values")
        .items()
        .iter()
        .map(|v| Value {
            descriptor: v.field("descriptor").text(),
            setting: v.field("setting").text(),
            bytes: hex(&v.field("value_hex").text()),
            valid: v.field("valid").flag(),
            rule: v.field("rule").text(),
        })
        .collect()
}

pub(super) fn hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    Text(String),
    Flag(bool),
}

impl Json {
    fn field(&self, name: &str) -> &Json {
        match self {
            Json::Object(fields) => &fields.iter().find(|(n, _)| n == name).unwrap().1,
            _ => panic!("not an object"),
        }
    }

    fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => panic!("not an array"),
        }
    }

    fn text(&self) -> String {
        match self {
            Json::Text(text) => text.clone(),
            _ => panic!("not a string"),
        }
    }

    fn flag(&self) -> bool {
        matches!(self, Json::Flag(true))
    }
}

fn read() -> Json {
    let text = include_str!("../../../../../vectors/workbench-vectors.json");
    Reader {
        bytes: text.as_bytes(),
        at: 0,
    }
    .value()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        while self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.skip();
        assert_eq!(
            self.bytes[self.at], c,
            "json: {} expected at {}",
            c as char, self.at
        );
        self.at += 1;
    }

    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out = Vec::new();
        while self.bytes[self.at] != b'"' {
            if self.bytes[self.at] == b'\\' {
                self.at += 1;
                out.push(match self.bytes[self.at] {
                    b'n' => b'\n',
                    c => c,
                });
            } else {
                out.push(self.bytes[self.at]);
            }
            self.at += 1;
        }
        self.at += 1;
        String::from_utf8(out).unwrap()
    }

    fn value(&mut self) -> Json {
        self.skip();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut fields = Vec::new();
                loop {
                    self.skip();
                    if self.bytes[self.at] == b'}' {
                        self.at += 1;
                        return Json::Object(fields);
                    }
                    if !fields.is_empty() {
                        self.eat(b',');
                    }
                    let name = self.string();
                    self.eat(b':');
                    fields.push((name, self.value()));
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip();
                    if self.bytes[self.at] == b']' {
                        self.at += 1;
                        return Json::Array(items);
                    }
                    if !items.is_empty() {
                        self.eat(b',');
                    }
                    items.push(self.value());
                }
            }
            b'"' => Json::Text(self.string()),
            b't' => {
                self.at += 4;
                Json::Flag(true)
            }
            b'f' => {
                self.at += 5;
                Json::Flag(false)
            }
            other => panic!("json: unexpected {} at {}", other as char, self.at),
        }
    }
}
