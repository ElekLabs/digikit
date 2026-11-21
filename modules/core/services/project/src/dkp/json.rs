use std::string::String;
use std::vec::Vec;

/// A minimal JSON reader for the vector file (a service crate has no dependency on a JSON crate; this is the one of `dkrt_sdk::settings`'s tests, with booleans): objects, arrays, strings
/// without escapes, booleans, unsigned numbers.
#[derive(Debug)]
pub(super) enum Value {
    Null,
    Bool(bool),
    Num(u64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

static NULL: Value = Value::Null;

impl Value {
    pub(super) fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub(super) fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub(super) fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }
}

impl core::ops::Index<&str> for Value {
    type Output = Value;
    fn index(&self, k: &str) -> &Value {
        match self {
            Value::Obj(o) => o
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v)
                .unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl core::ops::Index<usize> for Value {
    type Output = Value;
    fn index(&self, i: usize) -> &Value {
        match self {
            Value::Arr(a) => a.get(i).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

pub(super) struct Parser<'a> {
    pub(super) b: &'a [u8],
    pub(super) at: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.b.len() && self.b[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }
    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(
            self.b[self.at], c,
            "json: expected {} at {}",
            c as char, self.at
        );
        self.at += 1;
    }
    fn string(&mut self) -> String {
        self.eat(b'"');
        let start = self.at;
        while self.b[self.at] != b'"' {
            assert_ne!(self.b[self.at], b'\\', "json: escapes are not read");
            self.at += 1;
        }
        let s = String::from_utf8(self.b[start..self.at].to_vec()).unwrap();
        self.at += 1;
        s
    }
    pub(super) fn value(&mut self) -> Value {
        self.ws();
        match self.b[self.at] {
            b'{' => {
                self.at += 1;
                let mut o = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.at] == b'}' {
                        self.at += 1;
                        return Value::Obj(o);
                    }
                    if !o.is_empty() {
                        self.eat(b',');
                    }
                    let k = self.string();
                    self.eat(b':');
                    o.push((k, self.value()));
                }
            }
            b'[' => {
                self.at += 1;
                let mut a = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.at] == b']' {
                        self.at += 1;
                        return Value::Arr(a);
                    }
                    if !a.is_empty() {
                        self.eat(b',');
                    }
                    a.push(self.value());
                }
            }
            b'"' => Value::Str(self.string()),
            b'n' => {
                self.at += 4;
                Value::Null
            }
            b't' => {
                self.at += 4;
                Value::Bool(true)
            }
            b'f' => {
                self.at += 5;
                Value::Bool(false)
            }
            _ => {
                let start = self.at;
                while self.at < self.b.len() && self.b[self.at].is_ascii_digit() {
                    self.at += 1;
                }
                Value::Num(
                    core::str::from_utf8(&self.b[start..self.at])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }
}
